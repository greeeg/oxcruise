// Wall time and peak memory: oxcruise vs dependency-cruiser on synthetic repos and on a real one.
//
//   cargo build --release -p oxcruise
//   node tasks/benchmark/run.mjs [--sizes 1000,5000,20000] [--runs 3] [--skip-depcruise]
//
// Peak RSS comes from getrusage(RUSAGE_CHILDREN) via python3 (Linux/macOS). Prints a Markdown table.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { cpus, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "../..");
const arg = (n, d) => (process.argv.includes(n) ? process.argv[process.argv.indexOf(n) + 1] : d);
const sizes = arg("--sizes", "1000,5000,20000").split(",").map(Number);
const runs = Number(arg("--runs", "3"));
const skipDepcruise = process.argv.includes("--skip-depcruise");

const oxcruise = join(repo, "target/release/oxcruise");
const depcruise = join(repo, "tasks/conformance/node_modules/dependency-cruiser/bin/dependency-cruiser.mjs");
if (!existsSync(oxcruise)) {
  console.error("build first: cargo build --release -p oxcruise");
  process.exit(2);
}
const haveDepcruise = !skipDepcruise && existsSync(depcruise);

// Runs argv[1:], discards its stdout, forwards its stderr, then prints the child's peak RSS (KB on
// Linux, bytes on macOS) as the last stderr line and exits with the child's status.
const RUSAGE = `
import resource, subprocess, sys
p = subprocess.run(sys.argv[1:], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
sys.stderr.buffer.write(p.stderr)
rss = resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss
sys.stderr.write("\\n%d\\n" % (rss // 1024 if sys.platform == "darwin" else rss))
sys.exit(p.returncode)
`;

/** Runs a command `runs` times; returns best wall time (s) and the largest peak RSS (MB). */
function measure(cmd, args, cwd) {
  let best = Infinity;
  let rss = 0;
  for (let i = 0; i < runs; i++) {
    const t0 = performance.now();
    const r = spawnSync("python3", ["-c", RUSAGE, cmd, ...args], { cwd, encoding: "utf8", maxBuffer: 1 << 30 });
    const secs = (performance.now() - t0) / 1000;
    const kb = Number((r.stderr ?? "").trim().split("\n").at(-1));
    // exit 1 = rule violations found, which is a normal result for these runs
    if (r.status !== 0 && r.status !== 1) throw new Error(`${cmd} exited ${r.status}\n${r.stderr.slice(-500)}`);
    best = Math.min(best, secs);
    rss = Math.max(rss, kb / 1024);
  }
  return { secs: best, mb: rss };
}

const rows = [];
function bench(name, cwd, entries, modules) {
  // the same rule set for both tools: no-circular, plus the graph itself (JSON output to a pipe)
  const cfg = join(tmpdir(), "oxcruise-bench.dependency-cruiser.json");
  execFileSync("node", ["-e", `require("fs").writeFileSync(${JSON.stringify(cfg)}, JSON.stringify({forbidden:[{name:"no-circular",severity:"error",from:{},to:{circular:true}}],options:{doNotFollow:{path:"node_modules"},tsPreCompilationDeps:false}}))`]);
  const ox = measure(oxcruise, ["--config", cfg, "-f", "json-depcruise", ...entries], cwd);
  const dc = haveDepcruise ? measure("node", [depcruise, "--config", cfg, "-T", "json", ...entries], cwd) : null;
  rows.push({ name, modules, ox, dc });
}

const tmp = mkdtempSync(join(tmpdir(), "oxcruise-bench-"));
try {
  for (const n of sizes) {
    const dir = join(tmp, `synthetic-${n}`);
    execFileSync("node", [join(here, "generate.mjs"), dir, String(n)], { stdio: "ignore" });
    bench(`synthetic ${n.toLocaleString("en")} modules (JS)`, dir, ["."], n);
  }
  const real = join(repo, "tasks/conformance/.cache/dependency-cruiser");
  if (existsSync(real)) bench("dependency-cruiser's own src/ (JS, real code)", real, ["src"], null);
} finally {
  rmSync(tmp, { recursive: true, force: true });
}

const fmt = (m) => (m ? `${m.secs.toFixed(2)} s / ${m.mb.toFixed(0)} MB` : "n/a");
console.log(`\nbest of ${runs} runs, ${cpus().length} cores (${cpus()[0].model.trim()}), node ${process.version}\n`);
console.log("| target | oxcruise (time / peak RSS) | dependency-cruiser | speedup |");
console.log("|---|---|---|---|");
for (const r of rows) {
  console.log(`| ${r.name} | ${fmt(r.ox)} | ${fmt(r.dc)} | ${r.dc ? `${(r.dc.secs / r.ox.secs).toFixed(0)}x` : ""} |`);
}
