/**
 * A layered layout for a small directed graph (about 40 boxes): importers on top, foundations at the
 * bottom. Pure and deterministic: the same input always gives the same picture.
 *
 * 1. Tarjan SCC, then the condensed graph. A cycle becomes one unit that stays in one layer.
 * 2. Longest-path layering of the units, sources on top.
 * 3. Dummy points for edges that span several layers.
 * 4. Barycenter sweeps order each layer; ties go by name.
 * 5. Positions on a grid: fixed row height, box widths from the caller.
 */

export interface Point {
  x: number;
  y: number;
}

export interface LayoutNode {
  /** Breaks ties, so the order never depends on the input order. */
  name: string;
  width: number;
}

export interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface LayoutOptions {
  rowHeight: number;
  /** Space between boxes in a layer. */
  gapX: number;
  /** Space between layers. */
  gapY: number;
  /** Space between a cycle's frame and its boxes. */
  framePad: number;
  sweeps: number;
  /** Put what is imported on top and the importers at the bottom, instead of the other way round. */
  reverse: boolean;
}

export const DEFAULT_LAYOUT: LayoutOptions = {
  rowHeight: 44,
  gapX: 28,
  gapY: 64,
  framePad: 12,
  sweeps: 4,
  reverse: false,
};

export interface Route {
  /** Where the line goes, from the importing box to the imported one. */
  points: Point[];
  /** Both ends are in one cycle: the line is drawn as a short arc instead of going down. */
  inCycle: boolean;
}

export interface Layout {
  boxes: Box[];
  layer: number[];
  /** One frame per cycle: the boxes it holds and its outline. */
  frames: { nodes: number[]; box: Box }[];
  /** One route per input edge; edges inside one node (or out of range) have no points. */
  routes: Route[];
  width: number;
  height: number;
}

/** Tarjan's strongly connected components, iterative. Components come out sinks first. */
export function stronglyConnected(n: number, succ: readonly (readonly number[])[]): number[] {
  const index = new Int32Array(n).fill(-1);
  const low = new Int32Array(n);
  const onStack = new Uint8Array(n);
  const comp = new Array<number>(n).fill(-1);
  const stack: number[] = [];
  let counter = 0;
  let comps = 0;
  for (let root = 0; root < n; root++) {
    if (index[root] >= 0) continue;
    const work: [number, number][] = [[root, 0]];
    index[root] = low[root] = counter++;
    stack.push(root);
    onStack[root] = 1;
    while (work.length > 0) {
      const top = work[work.length - 1];
      const v = top[0];
      if (top[1] < succ[v].length) {
        const w = succ[v][top[1]++];
        if (index[w] < 0) {
          index[w] = low[w] = counter++;
          stack.push(w);
          onStack[w] = 1;
          work.push([w, 0]);
        } else if (onStack[w]) {
          low[v] = Math.min(low[v], index[w]);
        }
      } else {
        work.pop();
        if (work.length > 0) {
          const p = work[work.length - 1][0];
          low[p] = Math.min(low[p], low[v]);
        }
        if (low[v] === index[v]) {
          let w: number;
          do {
            w = stack.pop()!;
            onStack[w] = 0;
            comp[w] = comps;
          } while (w !== v);
          comps++;
        }
      }
    }
  }
  return comp;
}

/** A unit of the layered graph: a box, a cycle of boxes, or a point that an edge passes through. */
interface Unit {
  layer: number;
  /** Input nodes; empty for a dummy. */
  members: number[];
  key: string;
  w: number;
  h: number;
  x: number;
}

/** A vertical-tangent curve through `points`. Used by the map for its lines. */
export function curvePath(points: readonly Point[]): string {
  if (points.length === 0) return "";
  let d = `M${r(points[0].x)},${r(points[0].y)}`;
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1];
    const b = points[i];
    const my = (a.y + b.y) / 2;
    d += `C${r(a.x)},${r(my)} ${r(b.x)},${r(my)} ${r(b.x)},${r(b.y)}`;
  }
  return d;
}

/** A short arc between two points, bulging to one side: the reverse line bulges to the other. */
export function arcPath(a: Point, b: Point, bulge = 0.25): string {
  const mx = (a.x + b.x) / 2;
  const my = (a.y + b.y) / 2;
  const cx = mx + (b.y - a.y) * bulge;
  const cy = my - (b.x - a.x) * bulge;
  return `M${r(a.x)},${r(a.y)}Q${r(cx)},${r(cy)} ${r(b.x)},${r(b.y)}`;
}

function r(v: number): string {
  return String(Math.round(v * 10) / 10);
}

function less(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

export function layoutLayers(
  nodes: readonly LayoutNode[],
  edges: readonly (readonly [number, number])[],
  options: Partial<LayoutOptions> = {},
): Layout {
  const o = { ...DEFAULT_LAYOUT, ...options };
  const n = nodes.length;
  const succ: number[][] = Array.from({ length: n }, () => []);
  for (const [s, t] of edges) {
    if (s !== t && s >= 0 && t >= 0 && s < n && t < n) succ[s].push(t);
  }
  const comp = stronglyConnected(n, succ);
  const nComps = comp.reduce((m, c) => Math.max(m, c + 1), 0);

  // longest-path layering on the condensed graph; Tarjan numbers sinks first, so go from the top id down
  const compSucc: Set<number>[] = Array.from({ length: nComps }, () => new Set());
  for (let v = 0; v < n; v++) {
    for (const w of succ[v]) if (comp[v] !== comp[w]) compSucc[comp[v]].add(comp[w]);
  }
  const compLayer = new Array<number>(nComps).fill(0);
  for (let c = nComps - 1; c >= 0; c--) {
    for (const d of compSucc[c]) compLayer[d] = Math.max(compLayer[d], compLayer[c] + 1);
  }

  // units: one per component, dummies appended
  const units: Unit[] = [];
  const unitOfComp: number[] = [];
  const members: number[][] = Array.from({ length: nComps }, () => []);
  for (let v = 0; v < n; v++) members[comp[v]].push(v);
  const cellOf = new Map<number, { cols: number; cellW: number }>();
  for (let c = 0; c < nComps; c++) {
    const ms = members[c].sort((a, b) => less(nodes[a].name, nodes[b].name) || a - b);
    let w: number;
    let h: number;
    if (ms.length === 1) {
      w = nodes[ms[0]].width;
      h = o.rowHeight;
    } else {
      const cols = Math.ceil(Math.sqrt(ms.length));
      const rows = Math.ceil(ms.length / cols);
      const cellW = Math.max(...ms.map((m) => nodes[m].width));
      cellOf.set(c, { cols, cellW });
      w = cols * cellW + (cols - 1) * o.gapX * 0.5 + 2 * o.framePad;
      h = rows * o.rowHeight + (rows - 1) * o.gapX * 0.5 + 2 * o.framePad;
    }
    unitOfComp[c] = units.length;
    units.push({ layer: compLayer[c], members: ms, key: nodes[ms[0]].name, w, h, x: 0 });
  }

  // edges between units, and the chain of dummies each one gets
  const chains = new Map<string, number[]>();
  const pairs: [number, number][] = [];
  for (let c = 0; c < nComps; c++) {
    for (const d of [...compSucc[c]].sort((a, b) => a - b)) {
      pairs.push([unitOfComp[c], unitOfComp[d]]);
    }
  }
  const down: number[][] = [];
  const up: number[][] = [];
  const link = (a: number, b: number) => {
    (down[a] ??= []).push(b);
    (up[b] ??= []).push(a);
  };
  for (const [a, b] of pairs) {
    const la = units[a].layer;
    const lb = units[b].layer;
    const chain: number[] = [];
    let prev = a;
    for (let l = la + 1; l < lb; l++) {
      const id = units.length;
      units.push({
        layer: l,
        members: [],
        key: `￿${units[a].key}\u0000${units[b].key}`,
        w: 10,
        h: 0,
        x: 0,
      });
      chain.push(id);
      link(prev, id);
      prev = id;
    }
    link(prev, b);
    chains.set(`${a}>${b}`, chain);
  }

  // order each layer
  const nLayers = units.reduce((m, u) => Math.max(m, u.layer + 1), 0);
  const layers: number[][] = Array.from({ length: nLayers }, () => []);
  units.forEach((u, i) => layers[u.layer].push(i));
  const pos = new Float64Array(units.length);
  const place = (list: number[]) => list.forEach((u, i) => (pos[u] = i));
  for (const list of layers) {
    list.sort((a, b) => less(units[a].key, units[b].key) || a - b);
    place(list);
  }
  const sweep = (list: number[], neighbours: number[][]) => {
    const bary = new Map<number, number>();
    for (const u of list) {
      const ns = neighbours[u];
      if (!ns || ns.length === 0) {
        bary.set(u, pos[u]);
        continue;
      }
      let sum = 0;
      for (const v of ns) sum += pos[v];
      bary.set(u, sum / ns.length);
    }
    list.sort((a, b) => bary.get(a)! - bary.get(b)! || less(units[a].key, units[b].key) || a - b);
    place(list);
  };
  for (let i = 0; i < o.sweeps; i++) {
    for (let l = 1; l < nLayers; l++) sweep(layers[l], up);
    for (let l = nLayers - 2; l >= 0; l--) sweep(layers[l], down);
  }

  // the order is done; the bands may now be turned upside down
  if (o.reverse) {
    layers.reverse();
    for (const u of units) u.layer = nLayers - 1 - u.layer;
  }

  // x and y: pack each layer, centre it on the widest one
  const rowW = layers.map(
    (list) => list.reduce((s, u) => s + units[u].w, 0) + Math.max(0, list.length - 1) * o.gapX,
  );
  const width = Math.max(0, ...rowW);
  const layerTop: number[] = [];
  const layerH: number[] = [];
  let y = 0;
  layers.forEach((list, l) => {
    let x = (width - rowW[l]) / 2;
    for (const u of list) {
      units[u].x = x;
      x += units[u].w + o.gapX;
    }
    layerTop[l] = y;
    layerH[l] = Math.max(o.rowHeight, ...list.map((u) => units[u].h));
    y += layerH[l] + o.gapY;
  });
  const height = Math.max(0, y - o.gapY);

  // boxes: a single box is centred in its band, the members of a cycle fill a grid inside its frame
  const boxes: Box[] = nodes.map(() => ({ x: 0, y: 0, w: 0, h: 0 }));
  const frames: Layout["frames"] = [];
  const layer = new Array<number>(n).fill(0);
  for (let c = 0; c < nComps; c++) {
    const u = units[unitOfComp[c]];
    const top = layerTop[u.layer] + (layerH[u.layer] - u.h) / 2;
    const ms = u.members;
    if (ms.length === 1) {
      boxes[ms[0]] = { x: u.x, y: top, w: u.w, h: u.h };
      layer[ms[0]] = u.layer;
      continue;
    }
    const { cols, cellW } = cellOf.get(c)!;
    frames.push({ nodes: ms, box: { x: u.x, y: top, w: u.w, h: u.h } });
    ms.forEach((m, i) => {
      const col = i % cols;
      const row = Math.floor(i / cols);
      boxes[m] = {
        x: u.x + o.framePad + col * (cellW + o.gapX * 0.5) + (cellW - nodes[m].width) / 2,
        y: top + o.framePad + row * (o.rowHeight + o.gapX * 0.5),
        w: nodes[m].width,
        h: o.rowHeight,
      };
      layer[m] = u.layer;
    });
  }

  // routes: through the dummies of the chain, with the ends spread along the sides of a box
  const routes: Route[] = edges.map(() => ({ points: [], inCycle: false }));
  const middle: Point[][] = edges.map(() => []);
  const outgoing: number[][] = Array.from({ length: n }, () => []);
  const incoming: number[][] = Array.from({ length: n }, () => []);
  edges.forEach(([s, t], e) => {
    if (s === t || s < 0 || t < 0 || s >= n || t >= n) return;
    if (comp[s] === comp[t]) {
      routes[e].inCycle = true;
      return;
    }
    const chain = chains.get(`${unitOfComp[comp[s]]}>${unitOfComp[comp[t]]}`) ?? [];
    for (const d of chain) {
      const x = units[d].x + units[d].w / 2;
      const top = { x, y: layerTop[units[d].layer] };
      const bottom = { x, y: layerTop[units[d].layer] + layerH[units[d].layer] };
      // the line crosses the band in the direction it travels
      middle[e].push(...(o.reverse ? [bottom, top] : [top, bottom]));
    }
    outgoing[s].push(e);
    incoming[t].push(e);
  });
  const centre = (b: Box) => b.x + b.w / 2;
  const startPt: (Point | undefined)[] = [];
  const endPt: (Point | undefined)[] = [];
  for (let v = 0; v < n; v++) {
    const b = boxes[v];
    const spread = (
      list: number[],
      key: (e: number) => number,
      edgeY: number,
      put: (e: number, p: Point) => void,
    ) => {
      list.sort((a, c) => key(a) - key(c) || a - c);
      list.forEach((e, i) => put(e, { x: b.x + (b.w * (i + 1)) / (list.length + 1), y: edgeY }));
    };
    spread(
      outgoing[v],
      (e) => (middle[e][0] ? middle[e][0].x : centre(boxes[edges[e][1]])),
      o.reverse ? b.y : b.y + b.h,
      (e, p) => (startPt[e] = p),
    );
    spread(
      incoming[v],
      (e) => (middle[e].length ? middle[e][middle[e].length - 1].x : centre(boxes[edges[e][0]])),
      o.reverse ? b.y + b.h : b.y,
      (e, p) => (endPt[e] = p),
    );
  }
  edges.forEach(([s, t], e) => {
    if (routes[e].inCycle) routes[e].points = sidePoints(boxes[s], boxes[t]);
    else if (startPt[e] && endPt[e]) routes[e].points = [startPt[e]!, ...middle[e], endPt[e]!];
  });

  return { boxes, layer, frames, routes, width, height };
}

/** The ends of a line between two boxes of one cycle: facing sides, so it never crosses a box. */
function sidePoints(a: Box, b: Box): Point[] {
  const ac = { x: a.x + a.w / 2, y: a.y + a.h / 2 };
  const bc = { x: b.x + b.w / 2, y: b.y + b.h / 2 };
  if (Math.abs(ac.y - bc.y) < Math.abs(ac.x - bc.x)) {
    return bc.x > ac.x
      ? [
          { x: a.x + a.w, y: ac.y },
          { x: b.x, y: bc.y },
        ]
      : [
          { x: a.x, y: ac.y },
          { x: b.x + b.w, y: bc.y },
        ];
  }
  return bc.y > ac.y
    ? [
        { x: ac.x, y: a.y + a.h },
        { x: bc.x, y: b.y },
      ]
    : [
        { x: ac.x, y: a.y },
        { x: bc.x, y: b.y + b.h },
      ];
}

/** About how wide `text` is at 12 px in the page font. Layout never measures the DOM, so it stays pure. */
export function textWidth(text: string): number {
  return Math.ceil(text.length * 6.6);
}

/** `text` cut to fit `width` pixels, with an ellipsis. */
export function clip(text: string, width: number): string {
  const max = Math.max(1, Math.floor(width / 6.6));
  return text.length <= max ? text : `${text.slice(0, Math.max(1, max - 1))}…`;
}
