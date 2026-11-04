import { groupByFolder, neighbourhood } from "./logic";
import type { Direction } from "./logic";
import { clip, textWidth } from "./layout";
import type { Box } from "./layout";
import type { Model } from "../data/model";
import { EDGE_CIRCULAR, EDGE_ERROR, EDGE_TYPE_ONLY, EDGE_WARN } from "../data/types";

/** Rows per column: the rest folds into a `+N more` row. */
export const COLUMN_LIMIT = 30;

export interface NbOptions {
  hops: number;
  dir: Direction;
  /** Follow the type-only imports. */
  types: boolean;
  /** List the packages one by one; otherwise a column folds them into one row. */
  pkgs: boolean;
  hidden?: ReadonlySet<number>;
  /** The modules of a violation's cycle: drawn as a chain in the middle column. */
  cycle?: readonly number[];
}

export type ItemKind = "module" | "header" | "more" | "packages";

export interface NbItem extends Box {
  id: string;
  kind: ItemKind;
  /** Negative: importers, 0: the focus, positive: imports. */
  col: number;
  label: string;
  /** For a module row. */
  module?: number;
  /** For a header: the folder. */
  dir?: number;
  /** For a folded row: the nodes it stands for. */
  members?: number[];
  focus?: boolean;
  /** Reached in both directions: in a cycle with the focus. */
  cyclic?: boolean;
}

export interface NbEdge {
  /** Indexes of items. */
  from: number;
  to: number;
  edge: number;
  /** Points back towards the focus or sideways: part of a cycle. */
  back: boolean;
  circular: boolean;
  error: boolean;
  warn: boolean;
  typeOnly: boolean;
  /** An SVG path. */
  d: string;
}

export interface NbColumn {
  col: number;
  x: number;
  w: number;
}

export interface Neighbourhood {
  focus: number;
  items: NbItem[];
  edges: NbEdge[];
  columns: NbColumn[];
  width: number;
  height: number;
}

const ROW_H = 26;
const HEADER_H = 18;
const ROW_GAP = 4;
const GROUP_GAP = 12;
const COLUMN_GAP = 96;
const MIN_W = 120;
const MAX_W = 260;

interface Row {
  kind: ItemKind;
  label: string;
  module?: number;
  dir?: number;
  members?: number[];
  cyclic?: boolean;
}

/** The rows of one column: modules grouped by folder under headers, at most `limit` of them, then folded rows. */
function columnRows(
  model: Model,
  modules: number[],
  cyclic: ReadonlySet<number>,
  pkgs: boolean,
  limit: number,
): Row[] {
  const kind = model.data.modules.kind;
  const local = modules.filter((m) => kind[m] === 0);
  const external = modules.filter((m) => kind[m] !== 0);
  const rows: Row[] = [];
  let used = 0;
  const groups = groupByFolder(model, local);
  const folded: { dir: number; modules: number[] }[] = [];
  for (const g of groups) {
    if (used >= limit) {
      folded.push(g);
      continue;
    }
    const take = Math.min(limit - used, g.modules.length);
    const sorted = [...g.modules].sort((a, b) =>
      model.data.modules.name[a].localeCompare(model.data.modules.name[b]),
    );
    rows.push({ kind: "header", label: model.dirPaths[g.dir] || "(project root)", dir: g.dir });
    for (const m of sorted.slice(0, take)) {
      rows.push({
        kind: "module",
        label: model.data.modules.name[m],
        module: m,
        cyclic: cyclic.has(m),
      });
    }
    used += take;
    if (take < sorted.length) {
      const rest = sorted.slice(take);
      rows.push({
        kind: "more",
        label: `+${rest.length} more in ${model.dirPaths[g.dir] || "(project root)"}`,
        members: rest.map((m) => model.nodeOfMod(m)),
        dir: g.dir,
      });
    }
  }
  if (folded.length > 0) {
    const rest = folded.flatMap((g) => g.modules);
    rows.push({
      kind: "more",
      label:
        folded.length === 1
          ? `+${rest.length} more in ${model.dirPaths[folded[0].dir] || "(project root)"}`
          : `+${rest.length} more in ${folded.length} folders`,
      members: rest.map((m) => model.nodeOfMod(m)),
    });
  }
  if (external.length > 0) {
    if (!pkgs) {
      rows.push({
        kind: "packages",
        label: `${external.length} package${external.length === 1 ? "" : "s"}`,
        members: external.map((m) => model.nodeOfMod(m)),
      });
    } else {
      rows.push({ kind: "header", label: "packages" });
      const label = (m: number) => {
        const p = model.data.modules.package[m];
        return p >= 0 ? model.data.packages[p] : model.data.modules.name[m];
      };
      const sorted = [...external].sort((a, b) => label(a).localeCompare(label(b)) || a - b);
      const room = Math.max(1, limit - used);
      for (const m of sorted.slice(0, room)) {
        rows.push({ kind: "module", label: label(m), module: m, cyclic: cyclic.has(m) });
      }
      if (sorted.length > room) {
        const rest = sorted.slice(room);
        rows.push({
          kind: "more",
          label: `+${rest.length} more packages`,
          members: rest.map((m) => model.nodeOfMod(m)),
        });
      }
    }
  }
  return rows;
}

/**
 * The module `focus` in the middle, what imports it in columns to the left by hop distance, and what it
 * imports in columns to the right. Pure: the same input gives the same picture.
 */
export function layoutNeighbourhood(model: Model, focus: number, opts: NbOptions): Neighbourhood {
  const { hops, dir, types, hidden } = opts;
  const cycle = opts.cycle ?? [];
  const hiddenMods = new Set<number>();
  if (hidden) {
    for (const n of hidden) {
      if (model.isDir(n)) continue;
      hiddenMods.add(model.modOf(n));
    }
  }
  const isHidden = (m: number): boolean => {
    if (hiddenMods.has(m)) return true;
    if (!hidden || hidden.size === 0) return false;
    for (let d = model.modDir[m]; d > 0; d = model.dirParent[d]) if (hidden.has(d)) return true;
    return false;
  };
  const skip = (e: number): boolean =>
    (!types && (model.edgeFlags[e] & EDGE_TYPE_ONLY) !== 0) ||
    isHidden(model.edgeFrom[e]) ||
    isHidden(model.edgeTo[e]);

  const fwd =
    dir === "importers"
      ? new Map([[focus, 0]])
      : neighbourhood(model, focus, hops, "imports", skip).dist;
  const bwd =
    dir === "imports"
      ? new Map([[focus, 0]])
      : neighbourhood(model, focus, hops, "importers", skip).dist;

  // a signed column for each module: imports are positive, importers negative, the focus 0
  const col = new Map<number, number>([[focus, 0]]);
  const cyclic = new Set<number>();
  for (const [m, d] of fwd) if (m !== focus) col.set(m, d);
  for (const [m, d] of bwd) {
    if (m === focus) continue;
    if (col.has(m)) cyclic.add(m);
    else col.set(m, -d);
  }
  // the cycle of a violation is a chain in the middle column, starting at the focus
  const at = cycle.indexOf(focus);
  const chain =
    at < 0 ? [] : [...cycle.slice(at), ...cycle.slice(0, at)].filter((m) => !isHidden(m));
  for (const m of chain) col.set(m, 0);
  if (!col.has(focus)) col.set(focus, 0);

  const byCol = new Map<number, number[]>();
  for (const [m, c] of col) {
    if (c === 0) continue;
    const list = byCol.get(c);
    if (list) list.push(m);
    else byCol.set(c, [m]);
  }
  const cols = [...byCol.keys(), 0].sort((a, b) => a - b);

  // rows of each column, and its width
  const rowsOf = new Map<number, Row[]>();
  const widthOf = new Map<number, number>();
  const name = model.data.modules.name;
  for (const c of cols) {
    let rows: Row[];
    if (c === 0) {
      const mods = chain.length > 0 ? chain : [focus];
      rows = mods.map((m) => ({ kind: "module" as const, label: name[m], module: m }));
    } else {
      rows = columnRows(model, byCol.get(c)!, cyclic, opts.pkgs, COLUMN_LIMIT);
    }
    rowsOf.set(c, rows);
    widthOf.set(c, Math.min(MAX_W, Math.max(MIN_W, ...rows.map((r) => textWidth(r.label) + 28))));
  }

  // geometry: columns left to right, each one centred on the tallest
  const heightOf = (rows: Row[]) =>
    rows.reduce(
      (h, r, i) => h + (r.kind === "header" ? HEADER_H + (i > 0 ? GROUP_GAP : 0) : ROW_H + ROW_GAP),
      0,
    );
  const height = Math.max(...cols.map((c) => heightOf(rowsOf.get(c)!)));
  const items: NbItem[] = [];
  const columns: NbColumn[] = [];
  const itemOf = new Map<number, number>();
  let x = 0;
  for (const c of cols) {
    const w = widthOf.get(c)!;
    columns.push({ col: c, x, w });
    const rows = rowsOf.get(c)!;
    let y = (height - heightOf(rows)) / 2;
    rows.forEach((r, i) => {
      if (r.kind === "header") {
        if (i > 0) y += GROUP_GAP;
        items.push({
          id: `h${c}:${i}`,
          kind: "header",
          col: c,
          label: clip(r.label, w),
          dir: r.dir,
          x,
          y,
          w,
          h: HEADER_H,
        });
        y += HEADER_H;
        return;
      }
      const id =
        r.kind === "module" ? `m${r.module}` : r.kind === "more" ? `more${c}:${i}` : `pkgs${c}`;
      if (r.module !== undefined) itemOf.set(r.module, items.length);
      items.push({
        id,
        kind: r.kind,
        col: c,
        label: clip(r.label, w - 20),
        module: r.module,
        dir: r.dir,
        members: r.members,
        focus: r.module === focus,
        cyclic: r.cyclic,
        x,
        y,
        w,
        h: ROW_H,
      });
      y += ROW_H + ROW_GAP;
    });
    x += w + COLUMN_GAP;
  }
  const width = Math.max(0, x - COLUMN_GAP);

  // edges between neighbouring columns, and those that touch the middle column
  const edges: NbEdge[] = [];
  const put = (e: number, back: boolean) => {
    const a = itemOf.get(model.edgeFrom[e]);
    const b = itemOf.get(model.edgeTo[e]);
    if (a === undefined || b === undefined) return;
    const f = model.edgeFlags[e];
    edges.push({
      from: a,
      to: b,
      edge: e,
      back,
      circular: (f & EDGE_CIRCULAR) !== 0,
      error: (f & EDGE_ERROR) !== 0,
      warn: (f & EDGE_WARN) !== 0 && (f & EDGE_ERROR) === 0,
      typeOnly: (f & EDGE_TYPE_ONLY) !== 0,
      d: edgePath(items[a], items[b], back),
    });
  };
  if (chain.length > 1) {
    chain.forEach((m, i) => {
      const next = chain[(i + 1) % chain.length];
      const e = model.findEdge(m, next);
      if (e >= 0) put(e, i === chain.length - 1);
    });
  }
  for (const [m, c] of col) {
    if (!itemOf.has(m)) continue;
    for (let i = model.outStart[m]; i < model.outStart[m + 1]; i++) {
      const e = model.outEdge[i];
      const t = model.edgeTo[e];
      const ct = col.get(t);
      if (ct === undefined || !itemOf.has(t) || skip(e)) continue;
      if (c === ct) continue;
      if (Math.abs(ct - c) !== 1 && c !== 0 && ct !== 0) continue;
      put(e, ct < c);
    }
  }
  return { focus, items, edges, columns, width, height };
}

/** A curve from the side of `a` that faces `b`; in one column, a bow on the right, or on the left for a back edge. */
function edgePath(a: NbItem, b: NbItem, back: boolean): string {
  const r1 = (v: number) => Math.round(v * 10) / 10;
  const y0 = a.y + a.h / 2;
  const y1 = b.y + b.h / 2;
  if (a.col === b.col) {
    const x = back ? a.x : a.x + a.w;
    const out = (back ? -1 : 1) * 28;
    return `M${r1(x)},${r1(y0)}C${r1(x + out)},${r1(y0)} ${r1(x + out)},${r1(y1)} ${r1(x)},${r1(y1)}`;
  }
  const right = b.col > a.col;
  const x0 = right ? a.x + a.w : a.x;
  const x1 = right ? b.x : b.x + b.w;
  const mx = (x0 + x1) / 2;
  return `M${r1(x0)},${r1(y0)}C${r1(mx)},${r1(y0)} ${r1(mx)},${r1(y1)} ${r1(x1)},${r1(y1)}`;
}
