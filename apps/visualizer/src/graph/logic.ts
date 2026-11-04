import type { Model } from "../data/model";
import { EDGE_CIRCULAR, EDGE_ERROR, EDGE_TYPE_ONLY, EDGE_WARN } from "../data/types";

/** Edges between two visible nodes, merged. */
export interface Link {
  s: number;
  t: number;
  count: number;
  /** How many of the merged edges are type-only. */
  typeOnly: number;
  circular: boolean;
  error: boolean;
  warn: boolean;
}

export type Direction = "imports" | "importers" | "both";

/** The modules in the subtree of folder `d`. */
export function modulesUnder(model: Model, d: number): number[] {
  const out: number[] = [];
  const stack = [d];
  while (stack.length > 0) {
    const c = stack.pop()!;
    out.push(...model.childMods[c]);
    stack.push(...model.childDirs[c]);
  }
  return out;
}

/** Merge `edges` into links between the nodes that `map` gives. Edges inside one node vanish. O(E). */
export function aggregate(
  model: Model,
  edges: Iterable<number>,
  map: (module: number) => number,
): Link[] {
  const n = model.nodeCount;
  const byKey = new Map<number, Link>();
  for (const e of edges) {
    const s = map(model.edgeFrom[e]);
    const t = map(model.edgeTo[e]);
    if (s === t || s < 0 || t < 0) continue;
    const key = s * n + t;
    let link = byKey.get(key);
    if (!link) {
      link = { s, t, count: 0, typeOnly: 0, circular: false, error: false, warn: false };
      byKey.set(key, link);
    }
    const f = model.edgeFlags[e];
    link.count++;
    if (f & EDGE_TYPE_ONLY) link.typeOnly++;
    if (f & EDGE_CIRCULAR) link.circular = true;
    if (f & EDGE_ERROR) link.error = true;
    else if (f & EDGE_WARN) link.warn = true;
  }
  return [...byKey.values()];
}

/** The module-level edges behind the link `s` → `t` (nodes that are on screen). */
export function linkEdges(model: Model, s: number, t: number, skipTypeOnly = false): number[] {
  const inside = (node: number, n: number) =>
    model.isDir(n) ? model.isUnder(node, n) : node === n;
  const sources = model.isDir(s) ? modulesUnder(model, s) : [model.modOf(s)];
  const out: number[] = [];
  for (const m of sources) {
    for (let i = model.outStart[m]; i < model.outStart[m + 1]; i++) {
      const e = model.outEdge[i];
      if (skipTypeOnly && model.edgeFlags[e] & EDGE_TYPE_ONLY) continue;
      if (inside(model.nodeOfMod(model.edgeTo[e]), t)) out.push(e);
    }
  }
  return out;
}

/** The node that stands for module `m` outside folder `d`: the sibling of `d` or of one of its ancestors. */
export function outsideNode(model: Model, d: number, m: number): number {
  let x = model.nodeOfMod(m);
  while (!model.isUnder(d, model.parent(x))) x = model.parent(x);
  return x;
}

export interface DepGroup {
  /** The folder or module at the other end. */
  node: number;
  edges: number[];
}

/** What a folder depends on and what uses it, grouped by the node at the other end, biggest first. */
export function boundaryDeps(model: Model, d: number): { out: DepGroup[]; in: DepGroup[] } {
  const out = new Map<number, number[]>();
  const into = new Map<number, number[]>();
  const add = (groups: Map<number, number[]>, node: number, e: number) => {
    const list = groups.get(node);
    if (list) list.push(e);
    else groups.set(node, [e]);
  };
  for (const m of modulesUnder(model, d)) {
    for (let i = model.outStart[m]; i < model.outStart[m + 1]; i++) {
      const e = model.outEdge[i];
      const to = model.edgeTo[e];
      if (!model.isUnder(model.nodeOfMod(to), d)) add(out, outsideNode(model, d, to), e);
    }
    for (let i = model.inStart[m]; i < model.inStart[m + 1]; i++) {
      const e = model.inEdge[i];
      const from = model.edgeFrom[e];
      if (!model.isUnder(model.nodeOfMod(from), d)) add(into, outsideNode(model, d, from), e);
    }
  }
  const sorted = (groups: Map<number, number[]>) =>
    [...groups]
      .map(([node, edges]) => ({ node, edges }))
      .sort((a, b) => b.edges.length - a.edges.length || a.node - b.node);
  return { out: sorted(out), in: sorted(into) };
}

/** Modules grouped by the folder they are in, folders in path order. */
export function groupByFolder(
  model: Model,
  modules: readonly number[],
): { dir: number; modules: number[] }[] {
  const by = new Map<number, number[]>();
  for (const m of modules) {
    const d = model.modDir[m];
    const list = by.get(d);
    if (list) list.push(m);
    else by.set(d, [m]);
  }
  return [...by]
    .map(([dir, list]) => ({ dir, modules: list }))
    .sort((a, b) => model.dirPaths[a.dir].localeCompare(model.dirPaths[b.dir]));
}

/** Modules within `hops` of `start` and the distance of each. */
export function neighbourhood(
  model: Model,
  start: number,
  hops: number,
  direction: Direction,
  skip?: (edge: number) => boolean,
): { dist: Map<number, number>; edges: number[] } {
  const dist = new Map<number, number>([[start, 0]]);
  const edges = new Set<number>();
  let frontier = [start];
  for (let d = 0; d < hops && frontier.length > 0; d++) {
    const next: number[] = [];
    for (const m of frontier) {
      const visit = (e: number, other: number) => {
        if (skip?.(e)) return;
        edges.add(e);
        if (!dist.has(other)) {
          dist.set(other, d + 1);
          next.push(other);
        }
      };
      if (direction !== "importers") {
        for (let i = model.outStart[m]; i < model.outStart[m + 1]; i++) {
          visit(model.outEdge[i], model.edgeTo[model.outEdge[i]]);
        }
      }
      if (direction !== "imports") {
        for (let i = model.inStart[m]; i < model.inStart[m + 1]; i++) {
          visit(model.inEdge[i], model.edgeFrom[model.inEdge[i]]);
        }
      }
    }
    frontier = next;
  }
  return { dist, edges: [...edges] };
}
