// Lazy facades over the flat graph buffer written by `graph_buffer.rs` (layout documented there).
// Sections are `Uint32Array` views over the buffer itself, so nothing is copied or decoded
// until a rule asks for it; strings are decoded on demand and memoised.

import { DEP_KINDS, type DepKind } from "./kinds.generated.ts";
import type { Dependency, Export, ExportKind, Graph, Loc, Module, ModuleKind } from "./types.ts";

const MAGIC = 0x5247584f;
const VERSION = 2;
const NO_PACKAGE = 0xffffffff;
const MODULE_KINDS: ModuleKind[] = ["source", "stub", "core", "unresolved"];
const HEADER_WORDS = 24;
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
  /** Exports and used names were extracted (`cruise({ symbols: true })`, or a symbol rule/plugin). */
  readonly symbols: boolean;
  private readonly decoder = new TextDecoder();
  private readonly strings: (string | undefined)[];

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
    this.strings = new Array(stringCount);
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
  /** Module a re-export reads from, or undefined. */
  exportFrom(x: number): number | undefined {
    const m = this.exports[x * 6 + 4]!;
    return m === NO_REF ? undefined : m;
  }
  exportImported(x: number): string | undefined {
    const s = this.exports[x * 6 + 5]!;
    return s === NO_REF ? undefined : this.string(s);
  }
  /** Names edge `e` uses; empty when it uses anything (see `edgeUsesAll`) or nothing. */
  edgeNames(e: number): string[] {
    const out: string[] = [];
    for (let i = this.nameOffsets[e]!; i < this.nameOffsets[e + 1]!; i++)
      out.push(this.string(this.names[i]!));
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
    return MODULE_KINDS[this.g.buf.moduleFlags(this.index) & 0xf] ?? "source";
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
          name: buf.exportName(x),
          kind: EXPORT_KINDS[flags & 3]!,
          type: (flags & 4) !== 0,
          used: (flags & 8) !== 0,
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

class DependencyFacade implements Dependency {
  private cycleCache: readonly Module[] | undefined | null = null;
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
    const [lo, hi] = this.g.buf.edgeKinds(this.index);
    const out: DepKind[] = [];
    for (let i = 0; i < DEP_KINDS.length; i++) {
      const word = i < 32 ? lo : hi;
      if ((word >>> (i % 32)) & 1) out.push(DEP_KINDS[i]!);
    }
    return out;
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
    const bit = DEP_KINDS.indexOf(kind);
    if (bit < 0) throw new Error(`unknown dependency kind \`${kind}\``);
    const [lo, hi] = this.g.buf.edgeKinds(this.index);
    return (((bit < 32 ? lo : hi) >>> (bit % 32)) & 1) !== 0;
  }
}

export class GraphFacade implements Graph {
  readonly buf: GraphBuffer;
  private moduleCache: (ModuleFacade | undefined)[];
  private edgeCache: (DependencyFacade | undefined)[];
  private byPath: Map<string, number> | undefined;
  private allModules: Module[] | undefined;
  private allEdges: Dependency[] | undefined;

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
    return Array.from({ length: this.buf.packageCount }, (_, i) => this.buf.packageName(i));
  }
  get folders(): readonly string[] {
    const folders = new Set<string>();
    for (const m of this.modules) {
      if (m.kind !== "source") continue;
      const slash = m.path.lastIndexOf("/");
      folders.add(slash < 0 ? "" : m.path.slice(0, slash));
    }
    return [...folders].sort();
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

  /**
   * Modules that import export `name` of `module`: those that use it, and those that re-export
   * it (`export { name } from`, `export *`), followed through the re-exports to their users.
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
      for (let i = buf.inOffsets[m]!; i < buf.inOffsets[m + 1]!; i++) {
        const e = buf.inEdges[i]!;
        const from = buf.edgeFrom(e);
        if (buf.edgeUsesAll(e)) {
          found.add(from);
        } else {
          const prefix = n + ".";
          if (buf.edgeNames(e).some((x) => x === n || (!whole && x.startsWith(prefix))))
            found.add(from);
        }
        // a module that re-exports `n` from `m` hands the question on
        const [start, end] = buf.exportRange(from);
        for (let x = start; x < end; x++) {
          if (buf.exportFrom(x) !== m) continue;
          const kind = EXPORT_KINDS[buf.exportFlags(x) & 3];
          const exported = buf.exportName(x);
          // the re-exporting module imports the name itself, and passes it on
          if (kind === "reexport" && buf.exportImported(x) === n) {
            found.add(from);
            visit(from, exported, whole);
          } else if (kind === "star" && n !== "default") {
            found.add(from);
            visit(from, n, whole);
          } else if (kind === "namespace") {
            found.add(from);
            visit(from, `${exported}.${n}`, whole);
            visit(from, exported, true);
          }
        }
      }
    };
    visit(module.index, name, false);
    found.delete(module.index);
    return [...found].sort((a, b) => a - b).map((m) => this.module_(m));
  }

  cycles(): Module[][] {
    const groups = new Map<number, number[]>();
    for (let m = 0; m < this.buf.moduleCount; m++) {
      const id = this.buf.scc[m]!;
      (groups.get(id) ?? groups.set(id, []).get(id)!).push(m);
    }
    const out: Module[][] = [];
    for (const members of groups.values()) {
      const selfLoop =
        members.length === 1 &&
        this.module_(members[0]!).dependencies.some((d) => d.to.index === members[0] && d.circular);
      if (members.length > 1 || selfLoop) out.push(members.map((m) => this.module_(m)));
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
