// Lazy facades over the flat graph buffer written by `graph_buffer.rs` (layout documented there).
// Sections are `Uint32Array` views over the buffer itself, so nothing is copied or decoded
// until a rule asks for it; strings are decoded on demand and memoised.

import { DEP_KINDS, type DepKind } from "./kinds.generated.ts";
import type { Dependency, Graph, Loc, Module, ModuleKind } from "./types.ts";

const MAGIC = 0x5247584f;
const VERSION = 1;
const NO_PACKAGE = 0xffffffff;
const MODULE_KINDS: ModuleKind[] = ["source", "stub", "core", "unresolved"];
const HEADER_WORDS = 16;

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
  private readonly decoder = new TextDecoder();
  private readonly strings: (string | undefined)[];

  constructor(source: Uint8Array) {
    // A Node Buffer can start at any byte of a pooled ArrayBuffer; Uint32Array needs 4-byte alignment.
    const aligned = source.byteOffset % 4 === 0 ? source : Uint8Array.from(source);
    this.bytes = aligned;
    this.words = new Uint32Array(aligned.buffer, aligned.byteOffset, Math.floor(aligned.byteLength / 4));
    const h = this.words;
    if (h.length < HEADER_WORDS || h[0] !== MAGIC) throw new Error("not an oxcruise graph buffer");
    if (h[1] !== VERSION) throw new Error(`unsupported graph buffer version ${h[1]} (expected ${VERSION})`);
    this.moduleCount = h[2]!;
    this.edgeCount = h[3]!;
    const stringCount = h[4]!;
    const packageCount = h[5]!;
    const view = (byteOffset: number, length: number) => this.words.subarray(byteOffset / 4, byteOffset / 4 + length);
    this.stringOffsets = view(h[6]!, stringCount + 1);
    this.stringBase = h[7]!;
    this.modules = view(h[8]!, this.moduleCount * 4);
    this.edges = view(h[9]!, this.edgeCount * 8);
    this.outOffsets = view(h[10]!, this.moduleCount + 1);
    this.inOffsets = view(h[11]!, this.moduleCount + 1);
    this.inEdges = view(h[12]!, this.edgeCount);
    this.scc = view(h[13]!, this.moduleCount);
    this.packageStrings = view(h[14]!, packageCount);
    this.strings = new Array(stringCount);
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
      if ((word >>> i % 32) & 1) out.push(DEP_KINDS[i]!);
    }
    return out;
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
    if (this.cycleCache === null) this.cycleCache = this.circular ? this.g.shortestCycle(this.index) : undefined;
    return this.cycleCache;
  }
  is(kind: DepKind): boolean {
    const bit = DEP_KINDS.indexOf(kind);
    if (bit < 0) throw new Error(`unknown dependency kind \`${kind}\``);
    const [lo, hi] = this.g.buf.edgeKinds(this.index);
    return (((bit < 32 ? lo : hi) >>> bit % 32) & 1) !== 0;
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
    return (this.allModules ??= Array.from({ length: this.buf.moduleCount }, (_, i) => this.module_(i)));
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
    for (let cur = a; cur !== b; ) {
      const e = via.get(cur)!;
      back.push(this.buf.edgeFrom(e));
      cur = this.buf.edgeFrom(e);
    }
    return [this.module_(a), ...back.reverse().map((m) => this.module_(m))];
  }
}
