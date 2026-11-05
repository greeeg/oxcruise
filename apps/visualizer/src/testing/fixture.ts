import type { VizData } from "../data/types";

/**
 * src/a.ts -> src/b.ts -> src/ui/c.ts -> src/a.ts (a cycle), a -> (external)/react/index.js,
 * lib/d.ts imports src/ui/c.ts. Module ids follow the order of `name`.
 */
export function smallData(): VizData {
  return {
    version: 1,
    root: "/p",
    generatedAt: 0,
    summary: { error: 1, warn: 1, suppressed: 0, totalModules: 5, totalDependencies: 5 },
    symbols: false,
    // 0 root, 1 (external), 2 react, 3 src, 4 ui, 5 lib
    dirs: {
      parent: [0, 0, 1, 0, 3, 0],
      name: ["", "(external)", "react", "src", "ui", "lib"],
    },
    modules: {
      dir: [3, 3, 4, 5, 2],
      name: ["a.ts", "b.ts", "c.ts", "d.ts", "index.js"],
      kind: [0, 0, 0, 0, 1],
      size: [100, 200, 300, 50, 0],
      flags: [0, 0, 0, 2, 0],
      package: [-1, -1, -1, -1, 0],
    },
    packages: ["react"],
    rules: ["circular-dependencies", "orphan-modules"],
    edges: {
      from: [0, 1, 2, 0, 3],
      to: [1, 2, 0, 4, 2],
      kinds: [16, 16, 16, 16, 1],
      flags: [1 | 4, 1 | 4, 1 | 4, 0, 2],
    },
    violations: [
      {
        rule: 0,
        severity: "error",
        kind: "cycle",
        from: 0,
        to: 1,
        message: "cycle",
        cycle: [0, 1, 2],
      },
      { rule: 1, severity: "warn", kind: "module", from: 3, message: "orphan" },
    ],
  };
}

/**
 * A project of `top` folders with `sub` folders each and `files` modules in every leaf folder, plus
 * `loose` modules in the root and `packages` packages. Edges are pseudo-random but fixed. For the
 * cap and speed tests.
 */
export function bigData(opts: {
  top: number;
  sub: number;
  files: number;
  loose?: number;
  packages?: number;
  edgesPerModule?: number;
}): VizData {
  const { top, sub, files, loose = 0, packages = 0, edgesPerModule = 6 } = opts;
  const dirName = [""];
  const dirParent = [0];
  dirName.push("(external)");
  dirParent.push(0);
  const pkgDir: number[] = [];
  for (let p = 0; p < packages; p++) {
    pkgDir.push(dirName.length);
    dirName.push(`pkg${p}`);
    dirParent.push(1);
  }
  const leaves: number[] = [];
  for (let t = 0; t < top; t++) {
    const td = dirName.length;
    dirName.push(`top${String(t).padStart(3, "0")}`);
    dirParent.push(0);
    for (let s = 0; s < sub; s++) {
      leaves.push(dirName.length);
      dirName.push(`sub${s}`);
      dirParent.push(td);
    }
  }
  const mDir: number[] = [];
  const mName: string[] = [];
  const mKind: number[] = [];
  const mSize: number[] = [];
  const mPkg: number[] = [];
  const add = (dir: number, name: string, kind: number, pkg: number) => {
    mDir.push(dir);
    mName.push(name);
    mKind.push(kind);
    mSize.push(kind === 0 ? 100 : 0);
    mPkg.push(pkg);
  };
  for (let i = 0; i < loose; i++) add(0, `loose${i}.ts`, 0, -1);
  for (const d of leaves) for (let f = 0; f < files; f++) add(d, `f${f}.ts`, 0, -1);
  const firstPkg = mDir.length;
  for (let p = 0; p < packages; p++) add(pkgDir[p], "index.js", 1, p);
  const local = firstPkg;
  const from: number[] = [];
  const to: number[] = [];
  let seed = 12345;
  const rand = (n: number) => {
    seed = (seed * 1103515245 + 12345) & 0x7fffffff;
    return seed % n;
  };
  for (let m = 0; m < local; m++) {
    for (let k = 0; k < edgesPerModule; k++) {
      // mostly towards a module with a higher id, so the graph is mostly a DAG
      const t =
        k === 0 && packages > 0
          ? firstPkg + rand(packages)
          : m + 1 + rand(Math.max(1, local - m - 1));
      if (t < mDir.length && t !== m) {
        from.push(m);
        to.push(t);
      }
    }
  }
  return {
    version: 1,
    root: "/p",
    generatedAt: 0,
    summary: {
      error: 0,
      warn: 0,
      suppressed: 0,
      totalModules: mDir.length,
      totalDependencies: from.length,
    },
    symbols: false,
    dirs: { parent: dirParent, name: dirName },
    modules: {
      dir: mDir,
      name: mName,
      kind: mKind,
      size: mSize,
      flags: mDir.map(() => 0),
      package: mPkg,
    },
    packages: Array.from({ length: packages }, (_, p) => `pkg${p}`),
    rules: [],
    edges: {
      from,
      to,
      kinds: from.map(() => 16),
      flags: from.map(() => 0),
    },
    violations: [],
  };
}
