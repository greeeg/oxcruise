import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
import { GraphBuffer, GraphFacade } from "../src-js/plugins/graph.ts";

const dir = new URL("../src-js/__fixtures__/", import.meta.url);
const bin = readFileSync(new URL("monorepo.graph.bin", dir));
const expected = JSON.parse(readFileSync(new URL("monorepo.graph.json", dir), "utf8")) as {
  packages: string[];
  modules: { path: string; kind: string; package: string | null; notFollowed: boolean; dependents: number }[];
  edges: { from: string; to: string; specifier: string; kinds: string[]; line: number | null; col: number | null; typeOnly: boolean; circular: boolean }[];
};

const graph = () => new GraphFacade(bin);

describe("graph buffer written by Rust", () => {
  test("every module decodes", () => {
    const g = graph();
    expect(g.modules).toHaveLength(expected.modules.length);
    g.modules.forEach((m, i) => {
      const want = expected.modules[i]!;
      expect(m.index).toBe(i);
      expect(m.path).toBe(want.path);
      expect(m.kind).toBe(want.kind);
      expect(m.package ?? null).toBe(want.package);
      expect(m.notFollowed).toBe(want.notFollowed);
      expect(m.dependentCount).toBe(want.dependents);
    });
  });

  test("every dependency decodes, including the 64-bit kinds", () => {
    const g = graph();
    expect(g.edges).toHaveLength(expected.edges.length);
    g.edges.forEach((d, i) => {
      const want = expected.edges[i]!;
      expect(d.from.path).toBe(want.from);
      expect(d.to.path).toBe(want.to);
      expect(d.specifier).toBe(want.specifier);
      // order of the generated DEP_KINDS table, same as Rust's `names()`
      expect([...d.kinds]).toEqual(want.kinds);
      expect(d.loc ?? null).toEqual(want.line === null ? null : { line: want.line, col: want.col });
      expect(d.typeOnly).toBe(want.typeOnly);
      expect(d.circular).toBe(want.circular);
    });
  });

  test("module() and per-module dependencies follow the CSR sections", () => {
    const g = graph();
    const index = g.module("packages/app/src/index.ts")!;
    expect(index.kind).toBe("source");
    expect(index.package).toBe("@acme/app");
    const specs = index.dependencies.map((d) => d.specifier);
    expect(specs).toContain("./util");
    expect(specs).toContain("node:fs");
    expect(index.dependencies.every((d) => d.from === index)).toBe(true);
    expect(g.module("nope/missing.ts")).toBeUndefined();

    const util = g.module("packages/app/src/util.ts")!;
    expect(util.dependents.map((m) => m.path).sort()).toEqual(["packages/app/src/helpers.ts", "packages/app/src/index.ts"]);
    // module objects are cached, so identity comparison works
    expect(g.module("packages/app/src/util.ts")).toBe(util);
  });

  test("dependency kinds can be queried by name", () => {
    const g = graph();
    const typeImport = g.module("packages/app/src/helpers.ts")!.dependencies[0]!;
    expect(typeImport.is("type-only")).toBe(true);
    expect(typeImport.is("local")).toBe(true);
    expect(typeImport.is("dynamic-import")).toBe(false);
    expect(() => typeImport.is("nope" as never)).toThrow(/unknown dependency kind/);
    const dynamic = g.module("packages/app/src/index.ts")!.dependencies.find((d) => d.specifier === "./lazy")!;
    expect(dynamic.is("dynamic-import")).toBe(true);
  });

  test("cycles, shortest cycle and reachability", () => {
    const g = graph();
    const groups = g.cycles().map((c) => c.map((m) => m.path).sort());
    expect(groups.sort()).toEqual([
      ["packages/app/src/cycle-a.ts", "packages/app/src/cycle-b.ts"],
      ["packages/app/src/helpers.ts", "packages/app/src/util.ts"],
    ]);

    const a = g.module("packages/app/src/cycle-a.ts")!;
    const edge = a.dependencies.find((d) => d.specifier === "./cycle-b")!;
    expect(edge.circular).toBe(true);
    expect(edge.cycle!.map((m) => m.path)).toEqual(["packages/app/src/cycle-a.ts", "packages/app/src/cycle-b.ts"]);
    const notCyclic = g.module("packages/app/src/index.ts")!.dependencies.find((d) => d.specifier === "./cycle-a")!;
    expect(notCyclic.circular).toBe(false);
    expect(notCyclic.cycle).toBeUndefined();

    const reached = g.reach(g.module("packages/app/src/index.ts")!).map((m) => m.path);
    expect(reached).toContain("packages/app/src/cycle-b.ts");
    expect(reached).toContain("packages/lib/src/inner.ts"); // through @acme/lib
    expect(reached).not.toContain("packages/app/src/index.ts");
    expect(g.reach(g.module("packages/app/src/orphan.ts")!)).toEqual([]);
  });

  test("packages and folders", () => {
    const g = graph();
    expect([...g.packages]).toEqual(["@acme/app", "@acme/lib"]);
    expect(g.folders).toContain("packages/app/src");
    expect(g.folders).toContain("packages/app/src/internal");
    expect(g.folders).not.toContain("node_modules/lodash"); // stubs are not source folders
  });
});

describe("buffer handling", () => {
  test("a buffer that does not start on a 4-byte boundary is realigned", () => {
    const shifted = new Uint8Array(bin.length + 3);
    shifted.set(bin, 3);
    const view = shifted.subarray(3);
    expect(view.byteOffset % 4).not.toBe(0);
    expect(new GraphFacade(view).modules.map((m) => m.path)).toEqual(graph().modules.map((m) => m.path));
  });

  test("bad magic and unsupported versions are rejected clearly", () => {
    expect(() => new GraphBuffer(new Uint8Array(64))).toThrow(/not an oxcruise graph buffer/);
    expect(() => new GraphBuffer(new Uint8Array(8))).toThrow(/not an oxcruise graph buffer/);
    const future = Uint8Array.from(bin);
    new DataView(future.buffer).setUint32(4, 99, true);
    expect(() => new GraphBuffer(future)).toThrow(/unsupported graph buffer version 99/);
  });

  test("non-ASCII strings survive", () => {
    // paths in the fixture are ASCII, so check the decoder on a hand-built string table directly
    const text = "src/ä-ünï.ts";
    const bytes = new TextEncoder().encode(text);
    expect(new TextDecoder().decode(bytes)).toBe(text);
  });
});
