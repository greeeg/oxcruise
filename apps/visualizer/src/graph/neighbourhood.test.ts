import { describe, expect, it } from "vitest";
import { bigData, smallData } from "../testing/fixture";
import { Model } from "../data/model";
import { COLUMN_LIMIT, layoutNeighbourhood } from "./neighbourhood";
import type { NbOptions } from "./neighbourhood";

const model = new Model(smallData());
const opts = (over: Partial<NbOptions> = {}): NbOptions => ({
  hops: 2,
  dir: "both",
  types: true,
  pkgs: true,
  ...over,
});
const colsOf = (nb: ReturnType<typeof layoutNeighbourhood>) =>
  Object.fromEntries(
    nb.items
      .filter((i) => i.kind === "module")
      .map((i) => [model.data.modules.name[i.module!], i.col]),
  );

describe("layoutNeighbourhood: columns", () => {
  it("puts importers on the left and imports on the right", () => {
    // d -> c -> a -> b, a -> react
    const nb = layoutNeighbourhood(model, 3, opts({ hops: 1 }));
    expect(colsOf(nb)).toEqual({ "d.ts": 0, "c.ts": 1 });
    const back = layoutNeighbourhood(model, 2, opts({ hops: 1, dir: "importers" }));
    expect(colsOf(back)).toEqual({ "c.ts": 0, "b.ts": -1, "d.ts": -1 });
  });

  it("gives each module a signed column by hop distance", () => {
    const nb = layoutNeighbourhood(model, 3, opts({ hops: 3, dir: "imports" }));
    expect(colsOf(nb)).toEqual({ "d.ts": 0, "c.ts": 1, "a.ts": 2, "b.ts": 3, "index.js": 3 });
    expect(nb.columns.map((c) => c.col)).toEqual([0, 1, 2, 3]);
    // columns go left to right without overlap
    for (let i = 1; i < nb.columns.length; i++) {
      expect(nb.columns[i].x).toBeGreaterThanOrEqual(nb.columns[i - 1].x + nb.columns[i - 1].w);
    }
  });

  it("follows the direction", () => {
    const imports = colsOf(layoutNeighbourhood(model, 0, opts({ hops: 1, dir: "imports" })));
    expect(Object.keys(imports).sort()).toEqual(["a.ts", "b.ts", "index.js"]);
    const importers = colsOf(layoutNeighbourhood(model, 0, opts({ hops: 1, dir: "importers" })));
    expect(importers).toEqual({ "a.ts": 0, "c.ts": -1 });
  });

  it("marks the modules found in both directions and puts them on the imports side", () => {
    const nb = layoutNeighbourhood(model, 0, opts());
    const cyclic = nb.items.filter((i) => i.cyclic).map((i) => model.data.modules.name[i.module!]);
    expect(cyclic.sort()).toEqual(["b.ts", "c.ts"]);
    expect(colsOf(nb)["c.ts"]).toBe(2);
    expect(colsOf(nb)["d.ts"]).toBe(-2);
  });

  it("skips type-only imports and hidden modules", () => {
    const nb = layoutNeighbourhood(model, 3, opts({ hops: 1, types: false }));
    expect(colsOf(nb)).toEqual({ "d.ts": 0 });
    const hidden = layoutNeighbourhood(model, 0, opts({ hidden: new Set([model.nodeOfMod(1)]) }));
    expect(colsOf(hidden)["b.ts"]).toBeUndefined();
    const folder = layoutNeighbourhood(model, 0, opts({ hidden: new Set([3 + 1]) }));
    expect(colsOf(folder)["c.ts"]).toBeUndefined();
  });
});

describe("layoutNeighbourhood: rows and edges", () => {
  it("groups the modules of a column by folder, under a header", () => {
    const nb = layoutNeighbourhood(model, 0, opts({ hops: 2, dir: "imports" }));
    const col2 = nb.items.filter((i) => i.col === 2);
    expect(col2.map((i) => [i.kind, i.label])).toEqual([
      ["header", "src/ui"],
      ["module", "c.ts"],
    ]);
    const col1 = nb.items.filter((i) => i.col === 1);
    expect(col1.map((i) => i.label)).toEqual(["src", "b.ts", "packages", "react"]);
  });

  it("folds the packages into one row unless pkgs is on", () => {
    const nb = layoutNeighbourhood(model, 0, opts({ hops: 1, dir: "imports", pkgs: false }));
    const col1 = nb.items.filter((i) => i.col === 1);
    expect(col1.map((i) => i.kind)).toEqual(["header", "module", "packages"]);
    expect(col1[2]).toMatchObject({ label: "1 package", members: [model.nodeOfMod(4)] });
  });

  it("draws the edges between neighbouring columns", () => {
    const nb = layoutNeighbourhood(model, 3, opts({ hops: 3, dir: "imports" }));
    const name = (i: number) => model.data.modules.name[nb.items[i].module!];
    expect(nb.edges.map((e) => `${name(e.from)}>${name(e.to)}`).sort()).toEqual([
      "a.ts>b.ts",
      "a.ts>index.js",
      "c.ts>a.ts",
      "d.ts>c.ts",
    ]);
    expect(nb.edges.every((e) => e.d.startsWith("M"))).toBe(true);
    const dc = nb.edges.find((e) => name(e.from) === "d.ts")!;
    expect(dc.typeOnly).toBe(true);
    expect(dc.back).toBe(false);
  });

  it("draws an import back to the focus as a back edge", () => {
    const nb = layoutNeighbourhood(model, 0, opts({ hops: 1 }));
    const name = (i: number) => model.data.modules.name[nb.items[i].module!];
    // c is an importer and b an import: a -> b goes right, c -> a goes right
    for (const e of nb.edges) expect(e.back).toBe(false);
    expect(nb.edges.map((e) => `${name(e.from)}>${name(e.to)}`).sort()).toEqual([
      "a.ts>b.ts",
      "a.ts>index.js",
      "c.ts>a.ts",
    ]);
    const cyc = layoutNeighbourhood(model, 0, opts({ hops: 2, dir: "imports" }));
    const back = cyc.edges
      .filter((e) => e.back)
      .map((e) => `${name2(cyc, e.from)}>${name2(cyc, e.to)}`);
    expect(back).toEqual(["c.ts>a.ts"]);
  });

  it("lays the cycle of a violation out as a chain, with a back edge from the last to the first", () => {
    const nb = layoutNeighbourhood(model, 1, opts({ hops: 1, cycle: [0, 1, 2] }));
    const mid = nb.items.filter((i) => i.col === 0);
    // starts at the focus: b, c, a
    expect(mid.map((i) => i.label)).toEqual(["b.ts", "c.ts", "a.ts"]);
    expect(mid[0].y).toBeLessThan(mid[1].y);
    expect(mid[1].y).toBeLessThan(mid[2].y);
    const chain = nb.edges.filter((e) => nb.items[e.from].col === 0 && nb.items[e.to].col === 0);
    expect(chain).toHaveLength(3);
    expect(chain.filter((e) => e.back)).toHaveLength(1);
    expect(name2(nb, chain.find((e) => e.back)!.from)).toBe("a.ts");
    expect(chain.every((e) => e.circular)).toBe(true);
  });
});

function name2(nb: ReturnType<typeof layoutNeighbourhood>, i: number): string {
  return nb.items[i].label;
}

describe("layoutNeighbourhood: the row cap", () => {
  const big = new Model(bigData({ top: 1, sub: 4, files: 20, edgesPerModule: 0 }));
  // make module 0 import all 80 others
  const star = (() => {
    const data = bigData({ top: 1, sub: 4, files: 20, edgesPerModule: 0 });
    for (let m = 1; m < data.modules.name.length; m++) {
      data.edges.from.push(0);
      data.edges.to.push(m);
      data.edges.kinds.push(16);
      data.edges.flags.push(0);
    }
    return new Model(data);
  })();

  it("shows at most COLUMN_LIMIT modules per column and folds the rest", () => {
    expect(big.nMods).toBe(80);
    const nb = layoutNeighbourhood(star, 0, opts({ hops: 1, dir: "imports" }));
    const col = nb.items.filter((i) => i.col === 1);
    expect(col.filter((i) => i.kind === "module")).toHaveLength(COLUMN_LIMIT);
    const more = col.filter((i) => i.kind === "more");
    expect(more.length).toBeGreaterThan(0);
    const folded = more.reduce((n, i) => n + i.members!.length, 0);
    expect(folded).toBe(79 - COLUMN_LIMIT);
    expect(more[0].label).toMatch(/^\+\d+ more in /);
  });

  it("does not overlap rows in a column", () => {
    const nb = layoutNeighbourhood(star, 0, opts({ hops: 1, dir: "imports" }));
    const col = nb.items.filter((i) => i.col === 1);
    for (let i = 1; i < col.length; i++)
      expect(col[i].y).toBeGreaterThanOrEqual(col[i - 1].y + col[i - 1].h);
  });

  it("is deterministic", () => {
    const a = layoutNeighbourhood(star, 0, opts({ hops: 1, dir: "imports" }));
    expect(layoutNeighbourhood(star, 0, opts({ hops: 1, dir: "imports" }))).toEqual(a);
  });
});
