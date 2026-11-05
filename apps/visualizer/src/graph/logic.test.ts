import { describe, expect, it } from "vitest";
import { smallData } from "../testing/fixture";
import { aggregate, boundaryDeps, groupByFolder, linkEdges, neighbourhood } from "./logic";
import { Model } from "../data/model";
import { focusPatch, scopeInfo } from "../state/scope";
import { DEFAULT_STATE, formatHash, parseHash } from "../state/state";

const model = new Model(smallData());
const mod = (m: number) => model.nodeOfMod(m);

describe("aggregate", () => {
  it("counts merged edges and keeps level", () => {
    // a, b -> 3; c, react -> 1: b->c and a->react merge, c->a goes back, a->b vanishes
    const links = aggregate(model, [0, 1, 2, 3], (m) => (m < 2 ? 3 : 1));
    const forward = links.find((l) => l.s === 3 && l.t === 1)!;
    expect(forward).toMatchObject({ count: 2, circular: true, error: true, typeOnly: 0 });
    expect(links.find((l) => l.s === 1 && l.t === 3)?.count).toBe(1);
    expect(links).toHaveLength(2);
    // type-only edges are counted
    expect(aggregate(model, [4], (m) => m)[0]).toMatchObject({
      count: 1,
      typeOnly: 1,
      circular: false,
    });
  });

  it("drops the edges that stay inside one node or leave the map", () => {
    expect(aggregate(model, [0], () => 7)).toEqual([]);
    expect(aggregate(model, [0], (m) => (m === 0 ? 1 : -1))).toEqual([]);
  });
});

describe("neighbourhood", () => {
  it("follows the chosen direction", () => {
    expect([...neighbourhood(model, 0, 1, "imports").dist.keys()].sort()).toEqual([0, 1, 4]);
    expect([...neighbourhood(model, 0, 1, "importers").dist.keys()].sort()).toEqual([0, 2]);
    expect([...neighbourhood(model, 0, 1, "both").dist.keys()].sort()).toEqual([0, 1, 2, 4]);
  });

  it("respects the hop limit", () => {
    expect(neighbourhood(model, 3, 1, "imports").dist.has(0)).toBe(false);
    expect(neighbourhood(model, 3, 2, "imports").dist.get(0)).toBe(2);
    expect(neighbourhood(model, 3, 3, "imports").dist.get(1)).toBe(3);
  });
});

describe("linkEdges", () => {
  it("lists the module pairs behind a link", () => {
    expect(linkEdges(model, 3, 1)).toEqual([3]);
    expect(linkEdges(model, 5, 3)).toEqual([4]);
    expect(linkEdges(model, mod(0), mod(1))).toEqual([0]);
    expect(linkEdges(model, 5, 3, true)).toEqual([]);
    expect(linkEdges(model, 3, 4)).toEqual([1]);
  });
});

describe("boundaryDeps", () => {
  it("groups the edges that cross a folder by the node at the other end", () => {
    const src = boundaryDeps(model, 3);
    expect(src.out.map((g) => [g.node, g.edges])).toEqual([[1, [3]]]);
    expect(src.in.map((g) => [g.node, g.edges])).toEqual([[5, [4]]]);
    const ui = boundaryDeps(model, 4);
    expect(ui.out.map((g) => g.node)).toEqual([mod(0)]);
    // b and lib both reach c once: ties go by node id
    expect(ui.in.map((g) => g.node)).toEqual([5, mod(1)]);
  });

  it("is empty for a folder with no outside edges", () => {
    expect(boundaryDeps(model, 0)).toEqual({ out: [], in: [] });
  });
});

describe("groupByFolder", () => {
  it("groups modules by folder in path order", () => {
    expect(groupByFolder(model, [3, 0, 1])).toEqual([
      { dir: 5, modules: [3] },
      { dir: 3, modules: [0, 1] },
    ]);
  });
});

describe("scope", () => {
  const state = { ...DEFAULT_STATE, view: "graph" as const };

  it("tells a folder from a module and the project", () => {
    expect(scopeInfo(model, state)).toEqual({ focus: undefined, cycle: [] });
    expect(scopeInfo(model, { ...state, focus: "src" })).toEqual({ focus: 3, cycle: [] });
    expect(scopeInfo(model, { ...state, focus: "src/a.ts", cycle: 0 })).toEqual({
      focus: mod(0),
      cycle: [0, 1, 2],
    });
    // a cycle only counts for a module
    expect(scopeInfo(model, { ...state, focus: "src", cycle: 0 }).cycle).toEqual([]);
    expect(scopeInfo(model, { ...state, focus: "nope" }).focus).toBeUndefined();
  });

  it("focuses a folder or a module and selects it", () => {
    expect(focusPatch(model, 3)).toMatchObject({ view: "graph", focus: "src", sel: "src" });
    expect(focusPatch(model, mod(3), { cycle: 1 })).toMatchObject({
      focus: "lib/d.ts",
      cycle: 1,
      box: undefined,
    });
  });

  it("leaves the focus", () => {
    expect(focusPatch(model, null)).toMatchObject({ focus: undefined, link: undefined });
    expect(focusPatch(model, 0)).toMatchObject({ focus: undefined });
  });
});

describe("hash state", () => {
  it("round-trips", () => {
    const s = {
      ...DEFAULT_STATE,
      view: "graph" as const,
      focus: "src/a b.ts",
      hops: 3,
      dir: "importers" as const,
      sel: "src",
      hide: [2, 7],
      link: "src>lib",
      box: "files",
      pkgs: false,
      types: false,
      cycle: 0,
    };
    expect(parseHash(formatHash(s))).toMatchObject(s);
  });

  it("writes hops and direction only for a module focus", () => {
    const s = { ...DEFAULT_STATE, view: "graph" as const, focus: "src", hops: 3 };
    const hash = formatHash(s, (p) => p.endsWith(".ts"));
    expect(hash).not.toContain("hops");
    expect(formatHash({ ...s, focus: "a.ts" }, (p) => p.endsWith(".ts"))).toContain("hops=3");
  });

  it("reads the old focus view as the graph", () => {
    expect(parseHash("#view=focus&focus=src%2Fa.ts&hops=1&dir=imports")).toMatchObject({
      view: "graph",
      focus: "src/a.ts",
      hops: 1,
      dir: "imports",
    });
  });

  it("reads an old treemap link as the map of that folder", () => {
    expect(parseHash("#view=treemap&path=src%2Fui&sel=src%2Fui")).toMatchObject({
      view: "graph",
      focus: "src/ui",
      sel: "src/ui",
    });
    expect(parseHash("#view=treemap")).toMatchObject({ view: "graph", focus: undefined });
  });

  it("ignores the open folders of an old link", () => {
    const s = parseHash("#view=graph&open=1,2,3&focus=src");
    expect(s).toMatchObject({ view: "graph", focus: "src" });
    expect("open" in s).toBe(false);
    expect(formatHash({ ...s, open: [1] } as never)).not.toContain("open");
  });

  it("falls back on bad input", () => {
    expect(parseHash("#view=nope&hops=9&dir=x")).toMatchObject({
      view: "graph",
      hops: 2,
      dir: "both",
      pkgs: true,
      types: true,
    });
    expect(parseHash("")).toMatchObject({ view: "graph" });
    expect(parseHash("#hide=")).toMatchObject({ hide: undefined });
  });
});
