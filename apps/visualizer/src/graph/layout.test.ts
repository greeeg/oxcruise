import { describe, expect, it } from "vitest";
import { arcPath, curvePath, layoutLayers, stronglyConnected } from "./layout";
import type { Box, LayoutNode } from "./layout";

const nodes = (...names: string[]): LayoutNode[] => names.map((name) => ({ name, width: 100 }));
const overlap = (a: Box, b: Box) =>
  a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;

describe("stronglyConnected", () => {
  it("groups the members of a cycle", () => {
    const comp = stronglyConnected(4, [[1], [2], [0, 3], []]);
    expect(comp[0]).toBe(comp[1]);
    expect(comp[1]).toBe(comp[2]);
    expect(comp[3]).not.toBe(comp[0]);
  });

  it("numbers the sinks first", () => {
    const comp = stronglyConnected(3, [[1], [2], []]);
    expect(comp[2]).toBeLessThan(comp[1]);
    expect(comp[1]).toBeLessThan(comp[0]);
  });
});

describe("layoutLayers", () => {
  const ns = nodes("app", "ui", "core", "util", "lib");
  const edges: [number, number][] = [
    [0, 1],
    [0, 2],
    [1, 2],
    [2, 3],
    [0, 3],
    [1, 4],
  ];

  it("puts importers above what they import", () => {
    const l = layoutLayers(ns, edges);
    for (const [s, t] of edges) expect(l.layer[s]).toBeLessThan(l.layer[t]);
    expect(l.layer[0]).toBe(0);
    expect(l.boxes[0].y).toBeLessThan(l.boxes[1].y);
    expect(l.boxes[1].y).toBeLessThan(l.boxes[2].y);
  });

  it("can put the importers at the bottom", () => {
    const l = layoutLayers(ns, edges, { reverse: true });
    for (const [s, t] of edges) expect(l.boxes[s].y).toBeGreaterThan(l.boxes[t].y);
    edges.forEach(([s, t], e) => {
      const pts = l.routes[e].points;
      // a line leaves the top of the importer and enters the bottom of the imported box
      expect(pts[0].y).toBe(l.boxes[s].y);
      expect(pts[pts.length - 1].y).toBe(l.boxes[t].y + l.boxes[t].h);
      for (let i = 1; i < pts.length; i++) expect(pts[i].y).toBeLessThanOrEqual(pts[i - 1].y);
    });
  });

  it("never overlaps two boxes", () => {
    const l = layoutLayers(ns, edges);
    for (let a = 0; a < ns.length; a++) {
      for (let b = a + 1; b < ns.length; b++) expect(overlap(l.boxes[a], l.boxes[b])).toBe(false);
    }
  });

  it("is deterministic and does not depend on the input order", () => {
    const a = layoutLayers(ns, edges);
    expect(layoutLayers(ns, edges)).toEqual(a);
    // the same graph with the nodes listed backwards gives the same boxes per name
    const order = [4, 3, 2, 1, 0];
    const rev = layoutLayers(
      order.map((i) => ns[i]),
      edges.map(([s, t]) => [order.indexOf(s), order.indexOf(t)] as [number, number]),
    );
    order.forEach((orig, i) => expect(rev.boxes[i]).toEqual(a.boxes[orig]));
  });

  it("routes every edge from the bottom of the importer to the top of the imported box", () => {
    const l = layoutLayers(ns, edges);
    edges.forEach(([s, t], e) => {
      const pts = l.routes[e].points;
      expect(pts.length).toBeGreaterThanOrEqual(2);
      expect(pts[0].y).toBe(l.boxes[s].y + l.boxes[s].h);
      expect(pts[pts.length - 1].y).toBe(l.boxes[t].y);
      expect(l.routes[e].inCycle).toBe(false);
    });
    // app -> util skips two layers: it passes through dummy points
    expect(l.routes[4].points.length).toBeGreaterThan(2);
  });

  it("groups a cycle in one layer and one frame", () => {
    const cyc = nodes("a", "b", "c", "d");
    const l = layoutLayers(cyc, [
      [0, 1],
      [1, 0],
      [1, 2],
      [3, 0],
    ]);
    expect(l.layer[0]).toBe(l.layer[1]);
    expect(l.layer[3]).toBeLessThan(l.layer[0]);
    expect(l.layer[2]).toBeGreaterThan(l.layer[1]);
    expect(l.frames).toHaveLength(1);
    expect(l.frames[0].nodes.sort()).toEqual([0, 1]);
    const f = l.frames[0].box;
    for (const m of [0, 1]) {
      const b = l.boxes[m];
      expect(b.x).toBeGreaterThanOrEqual(f.x);
      expect(b.x + b.w).toBeLessThanOrEqual(f.x + f.w);
      expect(b.y).toBeGreaterThanOrEqual(f.y);
      expect(b.y + b.h).toBeLessThanOrEqual(f.y + f.h);
    }
    expect(overlap(l.boxes[0], l.boxes[1])).toBe(false);
    expect(l.routes[0].inCycle).toBe(true);
    expect(l.routes[1].inCycle).toBe(true);
    expect(l.routes[2].inCycle).toBe(false);
  });

  it("handles no nodes and ignores edges that are out of range or loop", () => {
    expect(layoutLayers([], []).boxes).toEqual([]);
    const l = layoutLayers(nodes("a"), [
      [0, 0],
      [0, 5],
    ]);
    expect(l.routes.every((r) => r.points.length === 0)).toBe(true);
  });
});

describe("paths", () => {
  it("draws a curve through the points and an arc between two", () => {
    expect(curvePath([])).toBe("");
    expect(
      curvePath([
        { x: 0, y: 0 },
        { x: 10, y: 20 },
      ]),
    ).toBe("M0,0C0,10 10,10 10,20");
    const ab = arcPath({ x: 0, y: 0 }, { x: 10, y: 0 });
    const ba = arcPath({ x: 10, y: 0 }, { x: 0, y: 0 });
    expect(ab).not.toBe(ba);
  });
});
