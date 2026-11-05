import type { VizData } from "./types";

/**
 * The decoded payload. Folders and modules share one id space ("nodes"): folder `d` is node `d`,
 * module `m` is node `nDirs + m`. Edges are in CSR form so a walk allocates nothing.
 */
export class Model {
  readonly nDirs: number;
  readonly nMods: number;
  readonly dirParent: Int32Array;
  readonly modDir: Int32Array;
  readonly dirPaths: string[];
  readonly modPaths: string[];
  readonly childDirs: number[][];
  readonly childMods: number[][];
  /** Pre-order index of each folder, and the last pre-order index in its subtree. */
  readonly dirPre: Int32Array;
  readonly dirPost: Int32Array;
  /** Folders between a folder and the root; the root is 0. */
  readonly dirDepth: Int32Array;
  /** Rolled up over the whole subtree of each folder. */
  readonly dirModules: Int32Array;
  readonly dirViolations: Int32Array;
  readonly dirErrors: Int32Array;
  readonly modViolations: Int32Array;
  readonly modErrors: Int32Array;
  /** Edge ids that leave / enter each module, `outStart[m]..outStart[m + 1]`. */
  readonly outStart: Int32Array;
  readonly outEdge: Int32Array;
  readonly inStart: Int32Array;
  readonly inEdge: Int32Array;
  readonly edgeFrom: Int32Array;
  readonly edgeTo: Int32Array;
  readonly edgeFlags: Uint8Array;
  readonly edgeKinds: Uint16Array;
  readonly violationsOf: Map<number, number[]>;
  private readonly dirByPath: Map<string, number>;
  private readonly modByPath: Map<string, number>;
  /** Lower-case paths of all nodes, for the search. */
  private readonly lowerPaths: string[];

  constructor(readonly data: VizData) {
    const { dirs, modules, edges } = data;
    const nDirs = (this.nDirs = dirs.name.length);
    const nMods = (this.nMods = modules.name.length);
    this.dirParent = Int32Array.from(dirs.parent);
    this.modDir = Int32Array.from(modules.dir);

    this.dirPaths = new Array(nDirs);
    this.childDirs = Array.from({ length: nDirs }, () => []);
    this.childMods = Array.from({ length: nDirs }, () => []);
    this.dirPaths[0] = "";
    for (let d = 1; d < nDirs; d++) {
      const p = dirs.parent[d];
      this.dirPaths[d] = p === 0 ? dirs.name[d] : `${this.dirPaths[p]}/${dirs.name[d]}`;
      this.childDirs[p].push(d);
    }
    this.modPaths = new Array(nMods);
    for (let m = 0; m < nMods; m++) {
      const d = modules.dir[m];
      this.modPaths[m] = d === 0 ? modules.name[m] : `${this.dirPaths[d]}/${modules.name[m]}`;
      this.childMods[d].push(m);
    }
    // pre-order ranges: a folder holds exactly the folders whose index lies in [pre, post]
    this.dirPre = new Int32Array(nDirs);
    this.dirPost = new Int32Array(nDirs);
    this.dirDepth = new Int32Array(nDirs);
    let counter = 0;
    const stack: [number, number][] = [[0, 0]];
    while (stack.length > 0) {
      const top = stack[stack.length - 1];
      const [d, next] = top;
      if (next === 0) this.dirPre[d] = counter++;
      if (next < this.childDirs[d].length) {
        top[1]++;
        const c = this.childDirs[d][next];
        this.dirDepth[c] = this.dirDepth[d] + 1;
        stack.push([c, 0]);
      } else {
        this.dirPost[d] = counter - 1;
        stack.pop();
      }
    }
    this.dirByPath = new Map(this.dirPaths.map((p, i) => [p, i]));
    this.modByPath = new Map(this.modPaths.map((p, i) => [p, i]));
    this.lowerPaths = [...this.dirPaths, ...this.modPaths].map((p) => p.toLowerCase());

    // violations
    this.modViolations = new Int32Array(nMods);
    this.modErrors = new Int32Array(nMods);
    this.violationsOf = new Map();
    data.violations.forEach((v, i) => {
      if (v.from === undefined) return;
      this.modViolations[v.from]++;
      if (v.level === "error") this.modErrors[v.from]++;
      const list = this.violationsOf.get(v.from);
      if (list) list.push(i);
      else this.violationsOf.set(v.from, [i]);
    });

    // rolled-up counts: a folder always has a smaller index than its children
    this.dirModules = new Int32Array(nDirs);
    this.dirViolations = new Int32Array(nDirs);
    this.dirErrors = new Int32Array(nDirs);
    for (let m = 0; m < nMods; m++) {
      const d = modules.dir[m];
      this.dirModules[d]++;
      this.dirViolations[d] += this.modViolations[m];
      this.dirErrors[d] += this.modErrors[m];
    }
    for (let d = nDirs - 1; d > 0; d--) {
      const p = dirs.parent[d];
      this.dirModules[p] += this.dirModules[d];
      this.dirViolations[p] += this.dirViolations[d];
      this.dirErrors[p] += this.dirErrors[d];
    }

    // CSR adjacency
    const nEdges = edges.from.length;
    this.edgeFrom = Int32Array.from(edges.from);
    this.edgeTo = Int32Array.from(edges.to);
    this.edgeFlags = Uint8Array.from(edges.flags);
    this.edgeKinds = Uint16Array.from(edges.kinds);
    this.outStart = new Int32Array(nMods + 1);
    this.inStart = new Int32Array(nMods + 1);
    for (let e = 0; e < nEdges; e++) {
      this.outStart[this.edgeFrom[e] + 1]++;
      this.inStart[this.edgeTo[e] + 1]++;
    }
    for (let m = 0; m < nMods; m++) {
      this.outStart[m + 1] += this.outStart[m];
      this.inStart[m + 1] += this.inStart[m];
    }
    this.outEdge = new Int32Array(nEdges);
    this.inEdge = new Int32Array(nEdges);
    const outAt = this.outStart.slice(0, nMods);
    const inAt = this.inStart.slice(0, nMods);
    for (let e = 0; e < nEdges; e++) {
      this.outEdge[outAt[this.edgeFrom[e]]++] = e;
      this.inEdge[inAt[this.edgeTo[e]]++] = e;
    }
  }

  get nodeCount(): number {
    return this.nDirs + this.nMods;
  }
  isDir(node: number): boolean {
    return node < this.nDirs;
  }
  nodeOfMod(m: number): number {
    return this.nDirs + m;
  }
  modOf(node: number): number {
    return node - this.nDirs;
  }
  /** The parent folder of a node; -1 for the root. */
  parent(node: number): number {
    if (node < this.nDirs) return node === 0 ? -1 : this.dirParent[node];
    return this.modDir[node - this.nDirs];
  }
  /** True when `node` is the folder `dir` or lies inside it. O(1). */
  isUnder(node: number, dir: number): boolean {
    const d = node < this.nDirs ? node : this.modDir[node - this.nDirs];
    const pre = this.dirPre[d];
    return pre >= this.dirPre[dir] && pre <= this.dirPost[dir];
  }
  /** The folders that hold `node`, nearest first, ending with the root. Empty for the root. */
  ancestors(node: number): number[] {
    const out: number[] = [];
    for (let p = this.parent(node); p >= 0; p = this.parent(p)) out.push(p);
    return out;
  }
  path(node: number): string {
    return node < this.nDirs ? this.dirPaths[node] : this.modPaths[node - this.nDirs];
  }
  name(node: number): string {
    return node < this.nDirs
      ? this.data.dirs.name[node]
      : this.data.modules.name[node - this.nDirs];
  }
  /** The child of the root that holds `node`, or -1 for the root. Gives the colour. */
  topLevel(node: number): number {
    let n = node;
    let p = this.parent(n);
    if (p < 0) return -1;
    while (p > 0) {
      n = p;
      p = this.parent(n);
    }
    return n;
  }
  nodeByPath(path: string): number {
    const d = this.dirByPath.get(path);
    if (d !== undefined) return d;
    const m = this.modByPath.get(path);
    return m === undefined ? -1 : this.nDirs + m;
  }
  moduleByPath(path: string): number {
    return this.modByPath.get(path) ?? -1;
  }
  dirByPathOrRoot(path: string | undefined): number {
    return (path && this.dirByPath.get(path)) || 0;
  }
  findEdge(from: number, to: number): number {
    for (let i = this.outStart[from]; i < this.outStart[from + 1]; i++) {
      if (this.edgeTo[this.outEdge[i]] === to) return this.outEdge[i];
    }
    return -1;
  }
  /**
   * Folders and modules whose path contains every word of `query`, best first. A word that is the whole
   * name or starts it beats a word inside the name, which beats a word in a folder of the path. A plain
   * scan: fast enough for large graphs.
   */
  searchNodes(query: string, limit = 50): number[] {
    const parts = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (parts.length === 0) return [];
    const hits: { node: number; score: number }[] = [];
    for (let node = 1; node < this.nodeCount; node++) {
      const p = this.lowerPaths[node];
      const nameAt = p.lastIndexOf("/") + 1;
      let score = 0;
      for (const part of parts) {
        const i = p.indexOf(part);
        if (i < 0) {
          score = -1;
          break;
        }
        const inName = p.indexOf(part, nameAt);
        if (inName < 0) score += 4;
        else if (inName === nameAt) score += part.length === p.length - nameAt ? 0 : 1;
        else score += 2;
      }
      if (score >= 0) hits.push({ node, score: score * 10_000 + p.length });
    }
    hits.sort((a, b) => a.score - b.score || a.node - b.node);
    return hits.slice(0, limit).map((h) => h.node);
  }
}
