import { describe, expect, it } from "vitest";
import { bigData, smallData } from "../testing/fixture";
import { Model } from "../data/model";
import { mapBoxes, mapScope, MAX_BOXES, resolveFolder, weakThreshold } from "./map";
import type { VizData } from "../data/types";

const model = new Model(smallData());
const mod = (m: number) => model.nodeOfMod(m);
const labels = (s: { boxes: { label: string }[] }) => s.boxes.map((b) => b.label);
const pairs = (s: ReturnType<typeof mapScope>) =>
  s.links.map((l) => `${s.boxes[l.s].label}>${s.boxes[l.t].label}`).sort();

describe("mapScope: root", () => {
  const s = mapScope(model, 0);

  it("has the child folders as boxes and never the (external) folder", () => {
    expect(labels(s)).toEqual(["lib", "src"]);
    expect(s.boxes.map((b) => b.node)).toEqual([5, 3]);
    expect(s.boxes[1]).toMatchObject({ kind: "dir", modules: 3, errors: 1 });
  });

  it("merges the edges into one link per pair of boxes", () => {
    expect(pairs(s)).toEqual(["lib>src"]);
    // lib/d.ts imports src/ui/c.ts as a type
    expect(s.links[0]).toMatchObject({ count: 1, typeOnly: 1 });
  });

  it("puts the packages in a lane, not in the links", () => {
    expect(s.packages).toEqual([
      { name: "react", count: 1, module: 4, files: ["(external)/react/index.js"] },
    ]);
    expect(s.packageImports).toBe(1);
    expect(s.portsOut).toEqual([]);
    expect(s.portsIn).toEqual([]);
  });

  it("drops the packages lane and the type-only edges when asked", () => {
    expect(mapScope(model, 0, { pkgs: false }).packages).toEqual([]);
    // d -> c is type-only
    expect(mapScope(model, 0, { types: false }).links).toEqual([]);
  });

  it("drops hidden folders and modules", () => {
    const hidden = mapScope(model, 0, { hidden: new Set([3]) });
    // lib is the only folder left, so the map steps into it
    expect(hidden.folder).toBe(5);
    expect(labels(hidden)).toEqual(["d.ts"]);
    expect(hidden.links).toEqual([]);
    expect(hidden.packages).toEqual([]);
    const one = mapScope(model, 3, { hidden: new Set([mod(1)]) });
    expect(labels(one)).toEqual(["ui", "a.ts"]);
    expect(pairs(one)).toEqual(["ui>a.ts"]);
  });
});

describe("mapScope: a folder", () => {
  const s = mapScope(model, 3);

  it("has its folders and loose modules as boxes", () => {
    expect(labels(s)).toEqual(["ui", "a.ts", "b.ts"]);
    expect(s.boxes.map((b) => b.kind)).toEqual(["dir", "mod", "mod"]);
    expect(pairs(s)).toEqual(["a.ts>b.ts", "b.ts>ui", "ui>a.ts"]);
    expect(s.links.every((l) => l.circular)).toBe(true);
  });

  it("turns the imports that leave the scope into ports", () => {
    expect(s.portsOut).toEqual([]);
    // lib/d.ts imports src/ui/c.ts, a type-only import
    expect(s.portsIn).toHaveLength(1);
    expect(s.portsIn[0]).toMatchObject({ node: 5, label: "lib", count: 1 });
    expect(s.portsIn[0].boxes).toEqual([{ box: 0, count: 1 }]);
    expect(s.packages).toEqual([
      { name: "react", count: 1, module: 4, files: ["(external)/react/index.js"] },
    ]);
    expect(mapScope(model, 3, { types: false }).portsIn).toEqual([]);
  });

  it("folds ports beyond the limit into one", () => {
    const data = bigData({ top: 12, sub: 1, files: 1, edgesPerModule: 12 });
    const m = new Model(data);
    const top = mapScope(m, m.childDirs[0][5]);
    expect(top.portsOut.length + top.portsIn.length).toBeGreaterThan(0);
    for (const ports of [top.portsOut, top.portsIn]) {
      expect(ports.length).toBeLessThanOrEqual(9);
      if (ports.length === 9) {
        const folded = ports[8];
        expect(folded.node).toBe(-1);
        expect(folded.label).toMatch(/^\+\d+$/);
        expect(folded.nodes!.length).toBe(Number(folded.label.slice(1)));
      }
    }
  });
});

describe("resolveFolder", () => {
  // root > only > deep > (a.ts, b.ts)
  const data = (): VizData => ({
    ...smallData(),
    dirs: { parent: [0, 0, 1], name: ["", "only", "deep"] },
    modules: {
      dir: [2, 2],
      name: ["a.ts", "b.ts"],
      kind: [0, 0],
      size: [1, 1],
      flags: [0, 0],
      package: [-1, -1],
    },
    edges: { from: [0], to: [1], kinds: [16], flags: [0] },
    violations: [],
  });
  const chain = new Model(data());

  it("skips a folder that holds only one folder", () => {
    expect(resolveFolder(chain, 0)).toBe(2);
    expect(resolveFolder(chain, 1)).toBe(2);
    expect(resolveFolder(chain, 2)).toBe(2);
    expect(labels(mapScope(chain, 0))).toEqual(["a.ts", "b.ts"]);
    expect(mapScope(chain, 0).folder).toBe(2);
  });

  it("keeps a folder with files, with two folders, or the (external) folder next to one", () => {
    expect(resolveFolder(model, 3)).toBe(3);
    expect(resolveFolder(model, 0)).toBe(0);
  });

  it("names a box after the whole chain", () => {
    const wide = new Model({
      ...data(),
      dirs: { parent: [0, 0, 1, 0], name: ["", "only", "deep", "other"] },
      modules: { ...data().modules, dir: [2, 3] },
    });
    expect(labels({ boxes: mapBoxes(wide, 0) })).toEqual(["only/deep", "other"]);
    // the box stands for the first folder of the chain
    expect(mapBoxes(wide, 0)[0].node).toBe(1);
    // and the links still land in it
    expect(pairs(mapScope(wide, 0))).toEqual(["only/deep>other"]);
  });
});

describe("the box cap", () => {
  it("merges loose files first", () => {
    const m = new Model(bigData({ top: 10, sub: 1, files: 1, loose: 50 }));
    const s = mapScope(m, 0);
    expect(s.boxes).toHaveLength(11);
    const files = s.boxes.find((b) => b.kind === "files")!;
    expect(files.label).toBe("files (50)");
    expect(files.members).toHaveLength(50);
    expect(files.modules).toBe(50);
    expect(s.boxes.filter((b) => b.kind === "dir")).toHaveLength(10);
  });

  it("merges the smallest folders when that is not enough", () => {
    const m = new Model(bigData({ top: 100, sub: 1, files: 1, loose: 5 }));
    const s = mapScope(m, 0);
    expect(s.boxes.length).toBeLessThanOrEqual(MAX_BOXES);
    const files = s.boxes.find((b) => b.kind === "files");
    const folders = s.boxes.find((b) => b.kind === "folders")!;
    expect(files?.modules).toBe(5);
    expect(folders.label).toBe(`+${folders.members.length} folders (${folders.modules} modules)`);
    // every folder is in a box
    const members = s.boxes.flatMap((b) => b.members);
    expect(members.filter((n) => m.isDir(n))).toHaveLength(100);
    // and every module belongs to one box, so no import is lost
    const seen = new Set(members);
    expect(seen.size).toBe(members.length);
  });

  it("keeps the biggest folders", () => {
    const data = bigData({ top: 60, sub: 1, files: 1 });
    // the first folder gets five more modules
    const first = data.dirs.name.indexOf("top000") + 1;
    for (let i = 0; i < 5; i++) {
      data.modules.dir.push(first);
      data.modules.name.push(`extra${i}.ts`);
      data.modules.kind.push(0);
      data.modules.flags.push(0);
      data.modules.package.push(-1);
    }
    const s = mapScope(new Model(data), 0);
    expect(s.boxes.length).toBeLessThanOrEqual(MAX_BOXES);
    expect(s.boxes.some((b) => b.kind === "dir" && b.label === "top000/sub0")).toBe(true);
  });

  it("counts the links across pseudo-boxes", () => {
    const m = new Model(bigData({ top: 100, sub: 1, files: 2, loose: 5 }));
    const s = mapScope(m, 0);
    const total = s.links.reduce((n, l) => n + l.count, 0);
    // every local import between two different boxes is counted once
    expect(total).toBeGreaterThan(0);
    expect(total).toBeLessThanOrEqual(m.data.edges.from.length);
  });
});

describe("weakThreshold", () => {
  const link = (count: number) => ({
    s: 0,
    t: 1,
    count,
    typeOnly: 0,
    circular: false,
    error: false,
    warn: false,
  });
  it("hides nothing in a small scope and the weakest links in a big one", () => {
    expect(weakThreshold([link(1), link(1)], 150)).toBe(1);
    const links = Array.from({ length: 10 }, (_, i) => link(i + 1));
    expect(weakThreshold(links, 4)).toBe(7);
  });
});
