// Diffs oxcruise against dependency-cruiser 18.7 on a set of targets.
//
//   node --experimental-strip-types tasks/conformance/run.ts [--only name] [--bin path] [--verbose]
//
// dependency-cruiser reads a generated dependency-cruiser config; oxcruise reads what
// `oxcruise migrate --json` makes of that same config, so this checks migration as well as parity.
// oxcruise's native `json` output is mapped to dependency-cruiser's shape here and compared on
// modules, edges and dependency types. Violations are not compared: oxcruise's rule language is
// much smaller, and the rules that cannot be migrated are dropped with a warning.
// Known, intentional differences live in deviations.json.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const cache = join(here, ".cache");
const repo = join(cache, "dependency-cruiser");
const depcruiseBin = join(here, "node_modules/dependency-cruiser/bin/dependency-cruiser.mjs");
const args = process.argv.slice(2);
const flag = (n: string) => args.includes(n);
const opt = (n: string) => (args.includes(n) ? args[args.indexOf(n) + 1] : undefined);
const oxcruiseBin = resolve(opt("--bin") ?? join(here, "../../target/debug/oxcruise"));
const only = opt("--only");
const verbose = flag("--verbose");

if (!existsSync(repo)) {
  console.error(
    `missing ${repo}\nclone it first:\n  git clone --depth 1 --branch v18.7.0 https://github.com/sverweij/dependency-cruiser.git ${repo}`,
  );
  process.exit(2);
}

type Target = {
  needs?: string[];
  exclude?: string;
  /** Hand-written globs for an `exclude` regex that `migrate` cannot convert. */
  excludeGlobs?: string[];
  name: string;
  dir: string;
  entries: string[];
  exoticRequireStrings?: string[];
  doNotFollow?: string;
  maxDepth?: number;
};
type Deviation = { target?: string; kind: string; match: string; reason: string };
const targets: Target[] = JSON.parse(readFileSync(join(here, "targets.json"), "utf8"));
const deviations: Deviation[] = JSON.parse(readFileSync(join(here, "deviations.json"), "utf8"));

// Types that describe what a dependency points to, vs how it is written.
const CLASSIFICATION = new Set([
  "core",
  "local",
  "localmodule",
  "npm",
  "npm-dev",
  "npm-optional",
  "npm-peer",
  "npm-bundled",
  "npm-no-pkg",
  "npm-unknown",
  "deprecated",
  "unknown",
  "undetermined",
  "aliased",
  "aliased-subpath-import",
  "aliased-tsconfig",
  "aliased-tsconfig-base-url",
  "aliased-tsconfig-paths",
  "aliased-webpack",
  "aliased-workspace",
  "type-only",
  "pre-compilation-only",
]);

function config(t: Target) {
  const options: Record<string, unknown> = {
    doNotFollow: { path: t.doNotFollow ?? "node_modules" },
    tsPreCompilationDeps: true,
    detectJSDocImports: true,
    moduleSystems: ["es6", "cjs", "amd", "tsd"],
    exoticRequireStrings: t.exoticRequireStrings ?? [],
  };
  if (t.maxDepth) options.maxDepth = t.maxDepth;
  if (t.exclude) options.exclude = { path: t.exclude };
  const dir = join(repo, t.dir);
  if (existsSync(join(dir, "tsconfig.json"))) options.tsConfig = { fileName: "tsconfig.json" };
  return {
    forbidden: [
      { name: "no-circular", severity: "error", from: {}, to: { circular: true } },
      {
        name: "no-orphans",
        severity: "warn",
        from: { orphan: true, pathNot: ["(^|/)[.][^/]+[.](js|cjs|mjs|ts|json)$", "[.]d[.]ts$"] },
        to: {},
      },
      { name: "not-to-unresolvable", severity: "error", from: {}, to: { couldNotResolve: true } },
      {
        name: "no-deprecated-core",
        severity: "warn",
        from: {},
        to: { dependencyTypes: ["core"], path: "^(punycode|sys|domain|constants)$" },
      },
      {
        name: "not-to-dev-dep",
        severity: "error",
        from: { path: "^(src|lib)" },
        to: { dependencyTypes: ["npm-dev"], dependencyTypesNot: ["type-only"] },
      },
      {
        name: "no-non-package-json",
        severity: "error",
        from: {},
        to: { dependencyTypes: ["npm-no-pkg", "npm-unknown"] },
      },
    ],
    options,
  };
}

type Dep = {
  resolved: string;
  module: string;
  dependencyTypes: string[];
  couldNotResolve?: boolean;
  coreModule?: boolean;
  dynamic?: boolean;
  moduleSystem?: string;
  /** oxcruise only: written as `new URL(.., import.meta.url)` or a worker. */
  url?: boolean;
};
type Mod = { source: string; dependencies: Dep[]; coreModule?: boolean };
type Result = { modules: Mod[] };

function run(cmd: string, argv: string[], cwd: string): string {
  try {
    return execFileSync(cmd, argv, {
      cwd,
      maxBuffer: 1 << 30,
      stdio: ["ignore", "pipe", "pipe"],
    }).toString();
  } catch (e) {
    // both tools exit non-zero when there are errors; the JSON is still on stdout
    const { stdout, stderr } = e as { stdout?: Buffer; stderr?: Buffer };
    if (stdout?.length) return stdout.toString();
    throw new Error(`${cmd} ${argv.join(" ")}\n${stderr?.toString() ?? e}`);
  }
}

// ---- oxcruise `json` -> dependency-cruiser's module shape ----------------------------------------

type OxDep = {
  to: string;
  to_kind: "source" | "stub" | "core" | "unresolved";
  specifier: string;
  kinds: string[];
  module_system: string;
};
type OxResult = { modules: { path: string; kind: string; dependencies: OxDep[] }[] };

// oxcruise-only concepts that dependency-cruiser does not know.
const OX_ONLY = new Set(["url", "worker", "call"]);
// How a dependency is written, as opposed to what it resolves to.
const SYNTAX = new Set([
  "import",
  "export",
  "require",
  "dynamic-import",
  "import-equals",
  "type-import",
  "amd-define",
  "amd-require",
  ...OX_ONLY,
]);

function fromOxcruise(json: string): Result {
  const ox = JSON.parse(json) as OxResult;
  const modules: Mod[] = ox.modules.map((m) => ({
    source: m.path,
    dependencies: m.dependencies.map((d) => {
      // dependency-cruiser reports only what an unresolvable dependency resolves to (`unknown`)
      const kinds = d.kinds.filter(
        (k) => !OX_ONLY.has(k) && !(d.to_kind === "unresolved" && SYNTAX.has(k)),
      );
      return {
        resolved: d.to,
        // dependency-cruiser strips the `node:` prefix of built-in modules
        module: d.to_kind === "core" ? d.specifier.replace(/^node:/, "") : d.specifier,
        dependencyTypes: kinds,
        couldNotResolve: d.to_kind === "unresolved",
        coreModule: d.to_kind === "core",
        dynamic: d.kinds.includes("dynamic-import"),
        moduleSystem: d.module_system,
        url: d.kinds.includes("url") || d.kinds.includes("worker"),
      };
    }),
  }));
  // unresolved specifiers are modules in dependency-cruiser's output
  const seen = new Set(modules.map((m) => m.source));
  for (const m of ox.modules)
    for (const d of m.dependencies)
      if (d.to_kind === "unresolved" && !seen.has(d.to)) {
        seen.add(d.to);
        modules.push({ source: d.to, dependencies: [] });
      }
  return { modules };
}

function facts(r: Result) {
  const modules = new Set<string>(r.modules.map((m) => m.source));
  const edges = new Map<string, Dep>();
  for (const m of r.modules)
    for (const d of m.dependencies) edges.set(`${m.source} -> ${d.resolved} (${d.module})`, d);
  return { modules, edges };
}

const normalizeSet = (a: string[], keep: (s: string) => boolean) => a.filter(keep).sort().join(",");

function deviation(target: string, kind: string, key: string): Deviation | undefined {
  return deviations.find(
    (d) => (!d.target || d.target === target) && d.kind === kind && new RegExp(d.match).test(key),
  );
}

mkdirSync(join(cache, "configs"), { recursive: true });
let unexplained = 0;
const report: Record<string, unknown> = {};

for (const t of targets.filter((t) => !only || t.name === only)) {
  const missing = (t.needs ?? []).filter((p) => !existsSync(join(here, "node_modules", p)));
  if (missing.length) {
    console.log(
      `skip ${t.name.padEnd(16)} dependency-cruiser needs ${missing.join(", ")} for this target (not installed)`,
    );
    continue;
  }
  const cfgPath = join(cache, "configs", `${t.name}.dependency-cruiser.json`);
  writeFileSync(cfgPath, JSON.stringify(config(t), null, 2));
  const cwd = join(repo, t.dir);

  // oxcruise gets what `migrate` makes of the very same config
  const oxCfgPath = join(cache, "configs", `${t.name}.oxcruiserc.json`);
  rmSync(oxCfgPath, { force: true }); // --migrate never overwrites
  const migrated = spawnSync(oxcruiseBin, [`--migrate=${cfgPath}`, "-c", oxCfgPath, "--cwd", cwd], {
    encoding: "utf8",
  });
  if (migrated.status !== 0) throw new Error(`migrate failed for ${t.name}:\n${migrated.stderr}`);
  const warnings = migrated.stderr.split("\n").filter((l) => l.startsWith("warning:"));
  if (t.excludeGlobs) {
    // an `exclude` regex that does not convert to a glob is written by hand
    const oxCfg = JSON.parse(readFileSync(oxCfgPath, "utf8"));
    oxCfg.ignorePatterns = t.excludeGlobs;
    writeFileSync(oxCfgPath, JSON.stringify(oxCfg, null, 2));
  }

  const dc: Result = JSON.parse(
    run("node", [depcruiseBin, "--config", cfgPath, "--output-type", "json", ...t.entries], cwd),
  );
  const ox = fromOxcruise(
    run(
      oxcruiseBin,
      [
        "--config",
        oxCfgPath,
        "-f",
        "json",
        "--cwd",
        cwd,
        ...(t.maxDepth ? ["--max-depth", String(t.maxDepth)] : []),
        ...t.entries,
      ],
      cwd,
    ),
  );

  const a = facts(dc),
    b = facts(ox);
  const diffs: { kind: string; key: string; detail?: string; reason?: string }[] = [];
  const add = (kind: string, key: string, detail?: string) => {
    const dev = deviation(t.name, kind, detail ? `${key} ${detail}` : key);
    diffs.push({ kind, key, detail, reason: dev?.reason });
  };

  // JSDoc imports are always on in oxcruise; dependency-cruiser needs `typescript` for them
  const isJsdoc = (d: Dep) => d.dependencyTypes.includes("jsdoc");
  const incoming = new Map<string, Dep[]>();
  for (const [k, d] of b.edges) {
    const to = k.split(" -> ")[1]!.replace(/ \(.*\)$/, "");
    incoming.set(to, [...(incoming.get(to) ?? []), d]);
  }
  // `new URL(..)` dependencies are always on in oxcruise; dependency-cruiser has none. Whatever is
  // reachable only through one is a consequence of it.
  const target = (k: string) => k.split(" -> ")[1]!.replace(/ \(.*\)$/, "");
  const source = (k: string) => k.split(" -> ")[0]!;
  const outgoing = new Map<string, string[]>();
  for (const k of b.edges.keys())
    outgoing.set(source(k), [...(outgoing.get(source(k)) ?? []), target(k)]);
  const viaUrl = new Set<string>();
  const queue: string[] = [];
  for (const [k, d] of b.edges)
    if (!a.edges.has(k) && d.url && !viaUrl.has(target(k))) {
      viaUrl.add(target(k));
      queue.push(target(k));
    }
  for (let m = queue.pop(); m !== undefined; m = queue.pop())
    for (const to of outgoing.get(m) ?? [])
      if (!viaUrl.has(to)) {
        viaUrl.add(to);
        queue.push(to);
      }
  for (const m of a.modules) if (!b.modules.has(m)) add("module-only-in-depcruise", m);
  for (const m of b.modules)
    if (!a.modules.has(m)) {
      const via = incoming.get(m) ?? [];
      add(
        viaUrl.has(m)
          ? "url-dependency-only-in-oxcruise"
          : via.length > 0 && via.every(isJsdoc)
            ? "jsdoc-import-only-in-oxcruise"
            : "module-only-in-oxcruise",
        m,
      );
    }
  for (const k of a.edges.keys()) if (!b.edges.has(k)) add("edge-only-in-depcruise", k);
  for (const [k, d] of b.edges)
    if (!a.edges.has(k))
      add(
        d.url || viaUrl.has(source(k))
          ? "url-dependency-only-in-oxcruise"
          : isJsdoc(d)
            ? "jsdoc-import-only-in-oxcruise"
            : "edge-only-in-oxcruise",
        k,
      );
  for (const [k, da] of a.edges) {
    const db = b.edges.get(k);
    if (!db) continue;
    if (isJsdoc(db) && !isJsdoc(da)) {
      // the same import is also written as a JSDoc import (type-only), which changes its kinds
      add("jsdoc-import-only-in-oxcruise", k);
      continue;
    }
    const ca = normalizeSet(da.dependencyTypes, (s) => CLASSIFICATION.has(s));
    const cb = normalizeSet(db.dependencyTypes, (s) => CLASSIFICATION.has(s));
    if (ca !== cb) add("classification-differs", k, `depcruise [${ca}] vs oxcruise [${cb}]`);
    // `exoticRequireStrings` and `calls` are one kind in oxcruise
    const syntax = (d: Dep) =>
      normalizeSet(
        d.dependencyTypes.map((s) => (s === "exotic-require" ? "call" : s)),
        (s) => !CLASSIFICATION.has(s) && s !== "call",
      );
    const sa = syntax(da);
    const sb = syntax(db);
    if (sa !== sb) add("syntax-differs", k, `depcruise [${sa}] vs oxcruise [${sb}]`);
    if ((da.moduleSystem ?? "") !== (db.moduleSystem ?? ""))
      add("module-system-differs", k, `${da.moduleSystem} vs ${db.moduleSystem}`);
  }

  const bad = diffs.filter((d) => !d.reason);
  unexplained += bad.length;
  report[t.name] = {
    modules: [a.modules.size, b.modules.size],
    edges: [a.edges.size, b.edges.size],
    migrationWarnings: warnings,
    diffs,
  };
  const explained = diffs.length - bad.length;
  console.log(
    `${bad.length === 0 ? "ok  " : "FAIL"} ${t.name.padEnd(16)} modules ${a.modules.size}/${b.modules.size}  edges ${a.edges.size}/${b.edges.size}  migrate warnings ${warnings.length}  diffs ${bad.length}${explained ? ` (+${explained} documented)` : ""}`,
  );
  for (const d of bad.slice(0, verbose ? 1000 : 8))
    console.log(`       ${d.kind}: ${d.key}${d.detail ? `\n         ${d.detail}` : ""}`);
  if (bad.length > 8 && !verbose) console.log(`       ... ${bad.length - 8} more (--verbose)`);
}

writeFileSync(join(cache, "report.json"), JSON.stringify(report, null, 2));
console.log(
  `\n${unexplained === 0 ? "conformant" : `${unexplained} unexplained difference(s)`}; full report: ${join(cache, "report.json")}`,
);
process.exit(unexplained === 0 ? 0 : 1);
