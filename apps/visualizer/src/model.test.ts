import { describe, expect, it } from "vitest";
import { smallData } from "./fixture";
import { Model } from "./model";

describe("Model", () => {
  const model = new Model(smallData());

  it("decodes paths", () => {
    expect(model.modPaths).toEqual([
      "src/a.ts",
      "src/b.ts",
      "src/ui/c.ts",
      "lib/d.ts",
      "(external)/react/index.js",
    ]);
    expect(model.nodeByPath("src/ui")).toBe(4);
    expect(model.nodeByPath("src/b.ts")).toBe(model.nodeOfMod(1));
    expect(model.nodeByPath("nope")).toBe(-1);
  });

  it("rolls counts up the folders", () => {
    expect([model.dirModules[0], model.dirModules[3], model.dirModules[4]]).toEqual([5, 3, 1]);
    expect(model.dirViolations[3]).toBe(1);
    expect(model.dirErrors[3]).toBe(1);
    expect(model.dirViolations[5]).toBe(1);
    expect(model.dirErrors[5]).toBe(0);
  });

  it("builds adjacency in both directions", () => {
    const outOf = (m: number) =>
      [...model.outEdge.slice(model.outStart[m], model.outStart[m + 1])].map(
        (e) => model.edgeTo[e],
      );
    const into = (m: number) =>
      [...model.inEdge.slice(model.inStart[m], model.inStart[m + 1])].map((e) => model.edgeFrom[e]);
    expect(outOf(0)).toEqual([1, 4]);
    expect(into(2)).toEqual([1, 3]);
    expect(model.findEdge(2, 0)).toBe(2);
    expect(model.findEdge(0, 2)).toBe(-1);
  });

  it("finds the top-level folder and parents", () => {
    expect(model.topLevel(model.nodeOfMod(2))).toBe(3);
    expect(model.topLevel(3)).toBe(3);
    expect(model.topLevel(0)).toBe(-1);
    expect(model.parent(0)).toBe(-1);
  });

  it("searches folders and modules, names first", () => {
    const path = (q: string) => model.searchNodes(q).map((n) => model.path(n));
    expect(path("c.ts")).toEqual(["src/ui/c.ts"]);
    expect(path("ui c.ts")).toEqual(["src/ui/c.ts"]);
    // a folder is found too, and a name beats a path
    expect(path("ui")).toEqual(["src/ui", "src/ui/c.ts"]);
    expect(path("src")[0]).toBe("src");
    expect(path("a")[0]).toBe("src/a.ts");
    expect(path("")).toEqual([]);
    expect(path("nope")).toEqual([]);
  });

  it("numbers the folders in pre-order", () => {
    expect([model.dirDepth[0], model.dirDepth[3], model.dirDepth[4], model.dirDepth[2]]).toEqual([
      0, 1, 2, 2,
    ]);
    expect(model.dirPre[0]).toBe(0);
    expect(model.dirPost[0]).toBe(5);
    expect(model.dirPost[3] - model.dirPre[3]).toBe(1);
  });

  it("tells what is under a folder", () => {
    const c = model.nodeOfMod(2);
    expect([model.isUnder(c, 4), model.isUnder(c, 3), model.isUnder(c, 0)]).toEqual([
      true,
      true,
      true,
    ]);
    expect(model.isUnder(c, 5)).toBe(false);
    expect(model.isUnder(4, 3)).toBe(true);
    expect(model.isUnder(3, 4)).toBe(false);
    expect(model.isUnder(3, 3)).toBe(true);
    expect(model.isUnder(model.nodeOfMod(4), 3)).toBe(false);
  });

  it("lists the ancestors, nearest first", () => {
    expect(model.ancestors(model.nodeOfMod(2))).toEqual([4, 3, 0]);
    expect(model.ancestors(2)).toEqual([1, 0]);
    expect(model.ancestors(0)).toEqual([]);
  });
});
