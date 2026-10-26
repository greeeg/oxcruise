// Diffs oxcruise against dependency-cruiser 18.7 on a set of targets.
//
//   node --experimental-strip-types tasks/conformance/run.ts [--only name] [--bin path] [--verbose]
//
// Both tools read the SAME dependency-cruiser config (oxcruise through its importer), so this
// checks migration as well as parity. Output is compared on modules, edges, dependency types
// and violations; known, intentional differences live in deviations.json.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import Ajv from "ajv";

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
  console.error(`missing ${repo}\nclone it first:\n  git clone --depth 1 --branch v18.7.0 https://github.com/sverweij/dependency-cruiser.git ${repo}`);
  process.exit(2);
}

type Target = { needs?: string[]; exclude?: string; name: string; dir: string; entries: string[]; exoticRequireStrings?: string[]; doNotFollow?: string; maxDepth?: number };
type Deviation = { target?: string; kind: string; match: string; reason: string };
const targets: Target[] = JSON.parse(readFileSync(join(here, "targets.json"), "utf8"));
const deviations: Deviation[] = JSON.parse(readFileSync(join(here, "deviations.json"), "utf8"));

// Types that describe what a dependency points to, vs how it is written.
const CLASSIFICATION = new Set([
  "core", "local", "localmodule", "npm", "npm-dev", "npm-optional", "npm-peer", "npm-bundled", "npm-no-pkg",
  "npm-unknown", "deprecated", "unknown", "undetermined", "aliased", "aliased-subpath-import", "aliased-tsconfig",
  "aliased-tsconfig-base-url", "aliased-tsconfig-paths", "aliased-webpack", "aliased-workspace", "type-only",
  "pre-compilation-only",
]);

function config(t: Target) {
  const options: Record<string, unknown> = {
    doNotFollow: { path: t.doNotFollow ?? "node_modules" },
    tsPreCompilationDeps: true,
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
      { name: "no-orphans", severity: "warn", from: { orphan: true, pathNot: ["(^|/)[.][^/]+[.](js|cjs|mjs|ts|json)$", "[.]d[.]ts$"] }, to: {} },
      { name: "not-to-unresolvable", severity: "error", from: {}, to: { couldNotResolve: true } },
      { name: "no-deprecated-core", severity: "warn", from: {}, to: { dependencyTypes: ["core"], path: "^(punycode|sys|domain|constants)$" } },
      { name: "not-to-dev-dep", severity: "error", from: { path: "^(src|lib)" }, to: { dependencyTypes: ["npm-dev"], dependencyTypesNot: ["type-only"] } },
      { name: "no-non-package-json", severity: "error", from: {}, to: { dependencyTypes: ["npm-no-pkg", "npm-unknown"] } },
    ],
    options,
  };
}

type Dep = { resolved: string; module: string; dependencyTypes: string[]; couldNotResolve?: boolean; coreModule?: boolean; dynamic?: boolean; moduleSystem?: string };
type Mod = { source: string; dependencies: Dep[]; coreModule?: boolean };
type Result = { modules: Mod[]; summary: { violations: { type: string; from: string; to: string; rule: { name: string; severity: string } }[] } };

function run(cmd: string, argv: string[], cwd: string): Result {
  try {
    return JSON.parse(execFileSync(cmd, argv, { cwd, maxBuffer: 1 << 30, stdio: ["ignore", "pipe", "pipe"] }).toString());
  } catch (e) {
    // both tools exit non-zero when there are errors; the JSON is still on stdout
    const { stdout, stderr } = e as { stdout?: Buffer; stderr?: Buffer };
    if (stdout?.length) return JSON.parse(stdout.toString());
    throw new Error(`${cmd} ${argv.join(" ")}\n${stderr?.toString() ?? e}`);
  }
}

const ajv = new Ajv({ strict: false, allErrors: true });
const validate = ajv.compile(JSON.parse(readFileSync(join(repo, "src/schema/cruise-result.schema.json"), "utf8")));

function facts(r: Result) {
  const modules = new Set<string>(r.modules.map((m) => m.source));
  const edges = new Map<string, Dep>();
  for (const m of r.modules) for (const d of m.dependencies) edges.set(`${m.source} -> ${d.resolved} (${d.module})`, d);
  const violations = new Set<string>(r.summary.violations.map((v) => `${v.rule.name}: ${v.from} -> ${v.to}`));
  return { modules, edges, violations };
}

const normalizeSet = (a: string[], keep: (s: string) => boolean) => a.filter(keep).sort().join(",");

function deviation(target: string, kind: string, key: string): Deviation | undefined {
  return deviations.find((d) => (!d.target || d.target === target) && d.kind === kind && new RegExp(d.match).test(key));
}

mkdirSync(join(cache, "configs"), { recursive: true });
let unexplained = 0;
const report: Record<string, unknown> = {};

for (const t of targets.filter((t) => !only || t.name === only)) {
  const missing = (t.needs ?? []).filter((p) => !existsSync(join(here, "node_modules", p)));
  if (missing.length) {
    console.log(`skip ${t.name.padEnd(16)} dependency-cruiser needs ${missing.join(", ")} for this target (not installed)`);
    continue;
  }
  const cfgPath = join(cache, "configs", `${t.name}.dependency-cruiser.json`);
  writeFileSync(cfgPath, JSON.stringify(config(t), null, 2));
  // oxcruise reads the very same dependency-cruiser config (through its importer). The wrapper only
  // switches off `new URL(..., import.meta.url)` dependencies, which depcruise does not have.
  const oxCfgPath = join(cache, "configs", `${t.name}.oxcruise.json`);
  writeFileSync(oxCfgPath, JSON.stringify({ extends: [`./${t.name}.dependency-cruiser.json`], extract: { skipUrls: true } }, null, 2));
  const cwd = join(repo, t.dir);

  const dc = run("node", [depcruiseBin, "--config", cfgPath, "--output-type", "json", ...t.entries], cwd);
  const ox = run(oxcruiseBin, ["--config", oxCfgPath, "-f", "json-depcruise", "--cwd", cwd, ...t.entries], cwd);

  const schemaErrors = (name: string, r: Result) => {
    if (validate(r)) return [];
    return (validate.errors ?? []).slice(0, 5).map((e) => `${name}: ${e.instancePath} ${e.message}`);
  };
  const schema = [...schemaErrors("depcruise", dc), ...schemaErrors("oxcruise", ox)];

  const a = facts(dc), b = facts(ox);
  const diffs: { kind: string; key: string; detail?: string; reason?: string }[] = [];
  const add = (kind: string, key: string, detail?: string) => {
    const dev = deviation(t.name, kind, detail ? `${key} ${detail}` : key);
    diffs.push({ kind, key, detail, reason: dev?.reason });
  };

  for (const m of a.modules) if (!b.modules.has(m)) add("module-only-in-depcruise", m);
  for (const m of b.modules) if (!a.modules.has(m)) add("module-only-in-oxcruise", m);
  for (const k of a.edges.keys()) if (!b.edges.has(k)) add("edge-only-in-depcruise", k);
  for (const k of b.edges.keys()) if (!a.edges.has(k)) add("edge-only-in-oxcruise", k);
  for (const [k, da] of a.edges) {
    const db = b.edges.get(k);
    if (!db) continue;
    const ca = normalizeSet(da.dependencyTypes, (s) => CLASSIFICATION.has(s));
    const cb = normalizeSet(db.dependencyTypes, (s) => CLASSIFICATION.has(s));
    if (ca !== cb) add("classification-differs", k, `depcruise [${ca}] vs oxcruise [${cb}]`);
    const sa = normalizeSet(da.dependencyTypes, (s) => !CLASSIFICATION.has(s));
    const sb = normalizeSet(db.dependencyTypes, (s) => !CLASSIFICATION.has(s));
    if (sa !== sb) add("syntax-differs", k, `depcruise [${sa}] vs oxcruise [${sb}]`);
    if ((da.moduleSystem ?? "") !== (db.moduleSystem ?? "")) add("module-system-differs", k, `${da.moduleSystem} vs ${db.moduleSystem}`);
  }
  for (const v of a.violations) if (!b.violations.has(v)) add("violation-only-in-depcruise", v);
  for (const v of b.violations) if (!a.violations.has(v)) add("violation-only-in-oxcruise", v);

  const bad = diffs.filter((d) => !d.reason);
  unexplained += bad.length + schema.length;
  report[t.name] = {
    modules: [a.modules.size, b.modules.size],
    edges: [a.edges.size, b.edges.size],
    violations: [a.violations.size, b.violations.size],
    schema,
    diffs,
  };
  const explained = diffs.length - bad.length;
  console.log(
    `${bad.length + schema.length === 0 ? "ok  " : "FAIL"} ${t.name.padEnd(16)} modules ${a.modules.size}/${b.modules.size}  edges ${a.edges.size}/${b.edges.size}  violations ${a.violations.size}/${b.violations.size}  diffs ${bad.length}${explained ? ` (+${explained} documented)` : ""}${schema.length ? `  schema errors ${schema.length}` : ""}`,
  );
  for (const d of bad.slice(0, verbose ? 1000 : 8)) console.log(`       ${d.kind}: ${d.key}${d.detail ? `\n         ${d.detail}` : ""}`);
  if (bad.length > 8 && !verbose) console.log(`       ... ${bad.length - 8} more (--verbose)`);
  for (const s of schema) console.log(`       schema: ${s}`);
}

writeFileSync(join(cache, "report.json"), JSON.stringify(report, null, 2));
console.log(`\n${unexplained === 0 ? "conformant" : `${unexplained} unexplained difference(s)`}; full report: ${join(cache, "report.json")}`);
process.exit(unexplained === 0 ? 0 : 1);
