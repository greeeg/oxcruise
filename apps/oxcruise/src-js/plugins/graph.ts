// Lazy facades over the flat graph buffer written by `graph_buffer.rs` (layout documented there).
// Sections are `Uint32Array` views over the buffer itself, so nothing is copied or decoded
// until a rule asks for it; strings are decoded on demand and memoised.

import { DEP_KINDS, type DepKind } from "./kinds.generated.ts";
import type { Dependency, Export, ExportKind, Graph, Loc, Module, ModuleKind } from "./types.ts";

const MAGIC = 0x5247584f;
const VERSION = 3;
const NO_PACKAGE = 0xffffffff;
const MODULE_KINDS: ModuleKind[] = ["local", "package", "builtin", "unresolved"];
const HEADER_WORDS = 26;
const NO_REF = 0xffffffff;
const EXPORT_KINDS: ExportKind[] = ["local", "reexport", "star", "namespace"];

/** Typed views over the buffer. */
export class GraphBuffer {
  readonly moduleCount: number;
  readonly edgeCount: number;
  private readonly words: Uint32Array;
  private readonly bytes: Uint8Array;
  private readonly stringOffsets: Uint32Array;
  private readonly stringBase: number;
  private readonly modules: Uint32Array;
  private readonly edges: Uint32Array;
  readonly outOffsets: Uint32Array;
  readonly inOffsets: Uint32Array;
  readonly inEdges: Uint32Array;
  readonly scc: Uint32Array;
  private readonly packageStrings: Uint32Array;
  private readonly exportOffsets: Uint32Array;
  private readonly exports: Uint32Array;
  private readonly nameOffsets: Uint32Array;
  private readonly names: Uint32Array;
  private readonly memberOffsets: Uint32Array;
  private readonly members: Uint32Array;
  /** Exports and used names were extracted (`cruise({ symbols: true })`, or a symbol rule/plugin). */
  readonly symbols: boolean;
  private readonly decoder = new TextDecoder();
  private readonly strings: (string | undefined)[];
  private readonly nameLists: (string[] | undefined)[];

  constructor(source: Uint8Array) {
    // A Node Buffer can start at any byte of a pooled ArrayBuffer; Uint32Array needs 4-byte alignment.
    const aligned = source.byteOffset % 4 === 0 ? source : Uint8Array.from(source);
    this.bytes = aligned;
    this.words = new Uint32Array(
      aligned.buffer,
      aligned.byteOffset,
      Math.floor(aligned.byteLength / 4),
    );
    const h = this.words;
    if (h.length < HEADER_WORDS || h[0] !== MAGIC) throw new Error("not an oxcruise graph buffer");
    if (h[1] !== VERSION)
      throw new Error(`unsupported graph buffer version ${h[1]} (expected ${VERSION})`);
    this.moduleCount = h[2]!;
    this.edgeCount = h[3]!;
    const stringCount = h[4]!;
    const packageCount = h[5]!;
    const view = (byteOffset: number, length: number) =>
      this.words.subarray(byteOffset / 4, byteOffset / 4 + length);
    this.stringOffsets = view(h[6]!, stringCount + 1);
    this.stringBase = h[7]!;
    this.modules = view(h[8]!, this.moduleCount * 4);
    this.edges = view(h[9]!, this.edgeCount * 8);
    this.outOffsets = view(h[10]!, this.moduleCount + 1);
    this.inOffsets = view(h[11]!, this.moduleCount + 1);
    this.inEdges = view(h[12]!, this.edgeCount);
    this.scc = view(h[13]!, this.moduleCount);
    this.packageStrings = view(h[14]!, packageCount);
    this.exportOffsets = view(h[18]!, this.moduleCount + 1);
    this.exports = view(h[19]!, h[16]! * 6);
    this.nameOffsets = view(h[20]!, this.edgeCount + 1);
    this.names = view(h[21]!, h[17]!);
    this.symbols = h[22] === 1;
    this.memberOffsets = view(h[23]!, h[16]! + 1);
    this.members = view(h[24]!, this.memberOffsets[h[16]!]!);
    this.strings = new Array(stringCount);
    this.nameLists = new Array(this.edgeCount);
  }

  /** Range of export numbers of module `m`. */
  exportRange(m: number): [start: number, end: number] {
    return [this.exportOffsets[m]!, this.exportOffsets[m + 1]!];
  }
  exportName(x: number): string {
    return this.string(this.exports[x * 6]!);
  }
  exportFlags(x: number): number {
    return this.exports[x * 6 + 1]!;
  }
  exportLine(x: number): number {
    return this.exports[x * 6 + 2]!;
  }
  exportCol(x: number): number {
    return this.exports[x * 6 + 3]!;
  }
  /** Every member of the export is used (`E.A` for any `A`); `exportMembers` is then empty. */
  exportUsesAllMembers(x: number): boolean {
    return (this.exportFlags(x) & 16) !== 0;
  }
  /** The members of the export that are read, sorted; empty when it is unused or used whole. */
  exportMembers(x: number): string[] {
    const out: string[] = [];
    for (let i = this.memberOffsets[x]!; i < this.memberOffsets[x + 1]!; i++)
      out.push(this.string(this.members[i]!));
    return out;
  }
  /** Module a re-export reads from, or undefined. */
  exportFrom(x: number): number | undefined {
    const m = this.exports[x * 6 + 4]!;
    return m === NO_REF ? undefined : m;
  }
  exportImported(x: number): string | undefined {
    const s = this.exports[x * 6 + 5]!;
    return s === NO_REF ? undefined : this.string(s);
  }
  /** Names edge `e` uses; empty when it uses anything (see `edgeUsesAll`) or nothing. Decoded once. */
  edgeNames(e: number): readonly string[] {
    let out = this.nameLists[e];
    if (out === undefined) {
      out = [];
      for (let i = this.nameOffsets[e]!; i < this.nameOffsets[e + 1]!; i++)
        out.push(this.string(this.names[i]!));
      this.nameLists[e] = out;
    }
    return out;
  }
  edgeUsesAll(e: number): boolean {
    return (this.edgeFlags(e) & 8) !== 0;
  }

  string(i: number): string {
    let s = this.strings[i];
    if (s === undefined) {
      const start = this.stringBase + this.stringOffsets[i]!;
      const end = this.stringBase + this.stringOffsets[i + 1]!;
      s = this.decoder.decode(this.bytes.subarray(start, end));
      this.strings[i] = s;
    }
    return s;
  }

  get packageCount(): number {
    return this.packageStrings.length;
  }
  packageName(i: number): string {
    return this.string(this.packageStrings[i]!);
  }

  modulePath(m: number): string {
    return this.string(this.modules[m * 4]!);
  }
  moduleFlags(m: number): number {
    return this.modules[m * 4 + 1]!;
  }
  modulePackage(m: number): number {
    return this.modules[m * 4 + 2]!;
  }
  moduleDependents(m: number): number {
    return this.modules[m * 4 + 3]!;
  }

  edgeFrom(e: number): number {
    return this.edges[e * 8]!;
  }
  edgeTo(e: number): number {
    return this.edges[e * 8 + 1]!;
  }
  edgeSpecifier(e: number): string {
    return this.string(this.edges[e * 8 + 2]!);
  }
  /** Kinds as two 32-bit halves of the 64-bit value. */
  edgeKinds(e: number): [lo: number, hi: number] {
    return [this.edges[e * 8 + 3]!, this.edges[e * 8 + 4]!];
  }
  edgeLine(e: number): number {
    return this.edges[e * 8 + 5]!;
  }
  edgeCol(e: number): number {
    return this.edges[e * 8 + 6]!;
  }
  edgeFlags(e: number): number {
    return this.edges[e * 8 + 7]!;
  }
}

class ModuleFacade implements Module {
  private deps: Dependency[] | undefined;
  private dependentList: Module[] | undefined;
  private exportList: Export[] | undefined;
  private readonly g: GraphFacade;
  readonly index: number;
  constructor(g: GraphFacade, index: number) {
    this.g = g;
    this.index = index;
  }

  get path(): string {
    return this.g.buf.modulePath(this.index);
  }
  get kind(): ModuleKind {
    return MODULE_KINDS[this.g.buf.moduleFlags(this.index) & 0xf] ?? "local";
  }
  get package(): string | undefined {
    const p = this.g.buf.modulePackage(this.index);
    return p === NO_PACKAGE ? undefined : this.g.buf.packageName(p);
  }
  get notFollowed(): boolean {
    return (this.g.buf.moduleFlags(this.index) & (1 << 4)) !== 0;
  }
  get dependencies(): readonly Dependency[] {
    if (!this.deps) {
      const { outOffsets } = this.g.buf;
      const start = outOffsets[this.index]!;
      const end = outOffsets[this.index + 1]!;
      this.deps = Array.from({ length: end - start }, (_, i) => this.g.edge(start + i));
    }
    return this.deps;
  }
  get dependents(): readonly Module[] {
    if (!this.dependentList) {
      const { inOffsets, inEdges } = this.g.buf;
      const seen = new Set<number>();
      for (let i = inOffsets[this.index]!; i < inOffsets[this.index + 1]!; i++) {
        const from = this.g.buf.edgeFrom(inEdges[i]!);
        if (from !== this.index) seen.add(from);
      }
      this.dependentList = [...seen].map((m) => this.g.module_(m));
    }
    return this.dependentList;
  }
  get dependentCount(): number {
    return this.g.buf.moduleDependents(this.index);
  }
  get exports(): readonly Export[] {
    if (!this.exportList) {
      const { buf } = this.g;
      const [start, end] = buf.exportRange(this.index);
      this.exportList = Array.from({ length: end - start }, (_, i) => {
        const x = start + i;
        const flags = buf.exportFlags(x);
        const from = buf.exportFrom(x);
        const line = buf.exportLine(x);
        return {
          module: this,
          name: buf.exportName(x),
          kind: EXPORT_KINDS[flags & 3]!,
          isType: (flags & 4) !== 0,
          used: (flags & 8) !== 0,
          usedMembers: (flags & 16) !== 0 ? "*" : buf.exportMembers(x),
          loc: line === 0 ? undefined : { line, col: buf.exportCol(x) },
          from: from === undefined ? undefined : this.g.module_(from),
          imported: buf.exportImported(x),
        };
      });
    }
    return this.exportList;
  }
  is(kind: ModuleKind): boolean {
    return this.kind === kind;
  }
}

/** Bit of each dependency kind in the graph buffer. */
const DEP_KIND_BIT = new Map<DepKind, number>(DEP_KINDS.map((kind, bit) => [kind, bit]));

class DependencyFacade implements Dependency {
  private cycleCache: readonly Module[] | undefined | null = null;
  private kindsCache: readonly DepKind[] | null = null;
  private readonly g: GraphFacade;
  readonly index: number;
  constructor(g: GraphFacade, index: number) {
    this.g = g;
    this.index = index;
  }

  get from(): Module {
    return this.g.module_(this.g.buf.edgeFrom(this.index));
  }
  get to(): Module {
    return this.g.module_(this.g.buf.edgeTo(this.index));
  }
  get specifier(): string {
    return this.g.buf.edgeSpecifier(this.index);
  }
  get kinds(): readonly DepKind[] {
    if (this.kindsCache === null) {
      const [lo, hi] = this.g.buf.edgeKinds(this.index);
      const out: DepKind[] = [];
      for (let i = 0; i < DEP_KINDS.length; i++) {
        const word = i < 32 ? lo : hi;
        if ((word >>> (i % 32)) & 1) out.push(DEP_KINDS[i]!);
      }
      this.kindsCache = out;
    }
    return this.kindsCache;
  }
  get names(): "*" | readonly string[] {
    return this.g.buf.edgeUsesAll(this.index) ? "*" : this.g.buf.edgeNames(this.index);
  }
  get loc(): Loc | undefined {
    const line = this.g.buf.edgeLine(this.index);
    return line === 0 ? undefined : { line, col: this.g.buf.edgeCol(this.index) };
  }
  get typeOnly(): boolean {
    return (this.g.buf.edgeFlags(this.index) & 1) !== 0;
  }
  get circular(): boolean {
    return (this.g.buf.edgeFlags(this.index) & 2) !== 0;
  }
  get unresolved(): boolean {
    return (this.g.buf.edgeFlags(this.index) & 4) !== 0;
  }
  get cycle(): readonly Module[] | undefined {
    if (this.cycleCache === null)
      this.cycleCache = this.circular ? this.g.shortestCycle(this.index) : undefined;
    return this.cycleCache;
  }
  is(kind: DepKind): boolean {
    const bit = DEP_KIND_BIT.get(kind);
    if (bit === undefined) throw new Error(`unknown dependency kind \`${kind}\``);
    const [lo, hi] = this.g.buf.edgeKinds(this.index);
    return (((bit < 32 ? lo : hi) >>> (bit % 32)) & 1) !== 0;
  }
}

/** An export of module `from` that reads from another module (`export { a } from`, `export *`, `export * as ns`). */
interface Reexporter {
  from: number;
  kind: ExportKind;
  /** The name `from` exports it under. */
  name: string;
  /** The name it reads (`a` in `export { a as b } from`). */
  imported: string | undefined;
}

export class GraphFacade implements Graph {
  readonly buf: GraphBuffer;
  private moduleCache: (ModuleFacade | undefined)[];
  private edgeCache: (DependencyFacade | undefined)[];
  private byPath: Map<string, number> | undefined;
  private reexporterIndex: Map<number, Reexporter[]> | undefined;
  private declared: (Set<string> | undefined)[] = [];
  private allModules: Module[] | undefined;
  private allEdges: Dependency[] | undefined;
  private allPackages: string[] | undefined;
  private allFolders: string[] | undefined;

  constructor(source: Uint8Array) {
    this.buf = new GraphBuffer(source);
    this.moduleCache = new Array(this.buf.moduleCount);
    this.edgeCache = new Array(this.buf.edgeCount);
  }

  /** Module facade by index (cached, so identity comparisons work). */
  module_(i: number): Module {
    return (this.moduleCache[i] ??= new ModuleFacade(this, i));
  }
  edge(i: number): Dependency {
    return (this.edgeCache[i] ??= new DependencyFacade(this, i));
  }

  get modules(): readonly Module[] {
    return (this.allModules ??= Array.from({ length: this.buf.moduleCount }, (_, i) =>
      this.module_(i),
    ));
  }
  get edges(): readonly Dependency[] {
    return (this.allEdges ??= Array.from({ length: this.buf.edgeCount }, (_, i) => this.edge(i)));
  }
  get packages(): readonly string[] {
    return (this.allPackages ??= Array.from({ length: this.buf.packageCount }, (_, i) =>
      this.buf.packageName(i),
    ));
  }
  get folders(): readonly string[] {
    if (this.allFolders) return this.allFolders;
    const folders = new Set<string>();
    for (const m of this.modules) {
      if (m.kind !== "local") continue;
      const slash = m.path.lastIndexOf("/");
      folders.add(slash < 0 ? "" : m.path.slice(0, slash));
    }
    return (this.allFolders = [...folders].sort());
  }

  module(path: string): Module | undefined {
    if (!this.byPath) {
      this.byPath = new Map();
      for (let i = 0; i < this.buf.moduleCount; i++) this.byPath.set(this.buf.modulePath(i), i);
    }
    const i = this.byPath.get(path);
    return i === undefined ? undefined : this.module_(i);
  }

  get symbols(): boolean {
    return this.buf.symbols;
  }

  /** The re-exports that read from each module, built once from the exports of every module. */
  private reexporters(source: number): readonly Reexporter[] {
    if (!this.reexporterIndex) {
      const index = new Map<number, Reexporter[]>();
      const { buf } = this;
      for (let m = 0; m < buf.moduleCount; m++) {
        const [start, end] = buf.exportRange(m);
        for (let x = start; x < end; x++) {
          const target = buf.exportFrom(x);
          if (target === undefined) continue;
          const entry: Reexporter = {
            from: m,
            kind: EXPORT_KINDS[buf.exportFlags(x) & 3]!,
            name: buf.exportName(x),
            imported: buf.exportImported(x),
          };
          const list = index.get(target);
          if (list) list.push(entry);
          else index.set(target, [entry]);
        }
      }
      this.reexporterIndex = index;
    }
    return this.reexporterIndex.get(source) ?? [];
  }

  /** Does module `m` export something called `name` itself? That hides what `export *` would supply. */
  private declares(m: number, name: string): boolean {
    let names = this.declared[m];
    if (!names) {
      names = new Set();
      const [start, end] = this.buf.exportRange(m);
      for (let x = start; x < end; x++) names.add(this.buf.exportName(x));
      this.declared[m] = names;
    }
    return names.has(name);
  }

  /**
   * Modules that import export `name` of `module`: those that use it, and those that re-export
   * it (`export { name } from`, `export *`), followed through the re-exports to their users.
   * A module that exports `name` itself hides the `export *` of another one, as in the
   * `no-unused-exports` rule. `name` may be `Export.member`.
   */
  importersOf(module: Module, name: string): Module[] {
    if (!this.buf.symbols)
      throw new Error("exports were not extracted; run cruise() with `symbols: true`");
    const { buf } = this;
    const found = new Set<number>();
    const seen = new Set<string>();
    // `whole`: only a use of the name itself counts, not of its members (`ns` but not `ns.a`)
    const visit = (m: number, n: string, whole: boolean) => {
      const key = `${m}\0${n}\0${whole}`;
      if (seen.has(key)) return;
      seen.add(key);
      const prefix = n + ".";
      for (let i = buf.inOffsets[m]!; i < buf.inOffsets[m + 1]!; i++) {
        const e = buf.inEdges[i]!;
        if (
          buf.edgeUsesAll(e) ||
          buf.edgeNames(e).some((x) => x === n || (!whole && x.startsWith(prefix)))
        )
          found.add(buf.edgeFrom(e));
      }
      // a module that re-exports `n` from `m` hands the question on
      const dot = n.indexOf(".");
      const head = dot < 0 ? n : n.slice(0, dot);
      const rest = dot < 0 ? "" : n.slice(dot + 1);
      for (const r of this.reexporters(m)) {
        if (r.kind === "reexport") {
          if ((r.imported ?? r.name) !== head) continue;
          found.add(r.from);
          visit(r.from, rest === "" ? r.name : `${r.name}.${rest}`, whole);
        } else if (r.kind === "star") {
          if (head === "default" || this.declares(r.from, head)) continue;
          found.add(r.from);
          visit(r.from, n, whole);
        } else if (r.kind === "namespace") {
          found.add(r.from);
          visit(r.from, `${r.name}.${n}`, whole);
          visit(r.from, r.name, true);
        }
      }
    };
    visit(module.index, name, false);
    found.delete(module.index);
    return [...found].sort((a, b) => a - b).map((m) => this.module_(m));
  }

  /** An edge from `m` to itself that lies on a cycle, read from the buffer without facades. */
  private hasSelfLoop(m: number): boolean {
    for (let e = this.buf.outOffsets[m]!; e < this.buf.outOffsets[m + 1]!; e++) {
      if (this.buf.edgeTo(e) === m && (this.buf.edgeFlags(e) & 2) !== 0) return true;
    }
    return false;
  }

  cycles(): Module[][] {
    const groups = new Map<number, number[]>();
    for (let m = 0; m < this.buf.moduleCount; m++) {
      const id = this.buf.scc[m]!;
      (groups.get(id) ?? groups.set(id, []).get(id)!).push(m);
    }
    const out: Module[][] = [];
    for (const members of groups.values()) {
      if (members.length > 1 || this.hasSelfLoop(members[0]!))
        out.push(members.map((m) => this.module_(m)));
    }
    return out;
  }

  reach(from: Module | readonly Module[]): Module[] {
    const starts = Array.isArray(from) ? from : [from as Module];
    const seen = new Set<number>(starts.map((m) => m.index));
    const queue = starts.map((m) => m.index);
    const out: Module[] = [];
    for (let head = 0; head < queue.length; head++) {
      const m = queue[head]!;
      for (let e = this.buf.outOffsets[m]!; e < this.buf.outOffsets[m + 1]!; e++) {
        const to = this.buf.edgeTo(e);
        if (!seen.has(to)) {
          seen.add(to);
          queue.push(to);
          out.push(this.module_(to));
        }
      }
    }
    return out;
  }

  /** BFS from `to` back to `from` inside the component, as in the Rust analysis. */
  shortestCycle(edge: number): Module[] | undefined {
    const a = this.buf.edgeFrom(edge);
    const b = this.buf.edgeTo(edge);
    if (a === b) return [this.module_(a)];
    const component = this.buf.scc[a]!;
    const via = new Map<number, number>(); // module -> edge used to reach it
    const seen = new Set<number>([b]);
    const queue = [b];
    for (let head = 0; head < queue.length && !seen.has(a); head++) {
      const m = queue[head]!;
      for (let e = this.buf.outOffsets[m]!; e < this.buf.outOffsets[m + 1]!; e++) {
        const to = this.buf.edgeTo(e);
        if (seen.has(to) || this.buf.scc[to] !== component) continue;
        seen.add(to);
        via.set(to, e);
        queue.push(to);
      }
    }
    if (!seen.has(a)) return undefined;
    const back: number[] = [];
    for (let cur = a; cur !== b;) {
      const e = via.get(cur)!;
      back.push(this.buf.edgeFrom(e));
      cur = this.buf.edgeFrom(e);
    }
    return [this.module_(a), ...back.reverse().map((m) => this.module_(m))];
  }
}
