// Packs the npm packages, installs the tarballs into a fresh project and runs `npx oxcruise` there.
//
//   pnpm -C apps/oxcruise build:binding:debug && pnpm -C apps/oxcruise build:js
//   node tasks/smoke/smoke.mjs
//
// Proves: the published file set is complete, the platform binding is found, a TypeScript config
// using defineConfig loads through the napi host (not a node subprocess), and a TS plugin runs.

import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(dirname(fileURLToPath(import.meta.url)), "../..");

function triple() {
  const { platform, arch } = process;
  if (platform === "linux") {
    const glibc = process.report?.getReport?.().header?.glibcVersionRuntime;
    return `linux-${arch}-${glibc ? "gnu" : "musl"}`;
  }
  if (platform === "darwin") return `darwin-${arch}`;
  if (platform === "win32") return `win32-${arch}-msvc`;
  throw new Error(`unsupported platform ${platform}`);
}

const t = triple();
const node = join(repo, "apps/oxcruise/src-js", `oxcruise.${t}.node`);
const dist = join(repo, "npm/oxcruise/dist/cli.js");
for (const [what, path, how] of [
  ["native addon", node, "pnpm -C apps/oxcruise build:binding:debug"],
  ["bundled JS", dist, "pnpm -C apps/oxcruise build:js"],
]) {
  if (!existsSync(path)) {
    console.error(`missing ${what} ${path}\nbuild it with: ${how}`);
    process.exit(2);
  }
}

const work = mkdtempSync(join(tmpdir(), "oxcruise-smoke-"));
const run = (cmd, args, opts = {}) =>
  execFileSync(cmd, args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      npm_config_update_notifier: "false",
      npm_config_fund: "false",
      npm_config_audit: "false",
    },
    ...opts,
  });
const attempt = (cmd, args, opts) => {
  try {
    return { code: 0, stdout: run(cmd, args, opts), stderr: "" };
  } catch (e) {
    return {
      code: e.status ?? 1,
      stdout: e.stdout?.toString() ?? "",
      stderr: e.stderr?.toString() ?? "",
    };
  }
};
let failures = 0;
const check = (name, ok, detail = "") => {
  console.log(`${ok ? "ok  " : "FAIL"} ${name}${ok ? "" : `\n       ${detail}`}`);
  if (!ok) failures++;
};

try {
  // 1. platform package gets the binary, then both are packed
  const platformDir = join(repo, "npm", t);
  copyFileSync(node, join(platformDir, `oxcruise.${t}.node`));
  const packs = join(work, "packs");
  mkdirSync(packs);
  for (const dir of [join(repo, "npm/oxcruise"), platformDir])
    run("npm", ["pack", "--pack-destination", packs], { cwd: dir });
  rmSync(join(platformDir, `oxcruise.${t}.node`));
  const tarballs = readdirSync(packs).map((f) => join(packs, f));
  check("packed the main and platform packages", tarballs.length === 2, tarballs.join(", "));

  // 2. a fresh project
  const proj = join(work, "project");
  const write = (p, c) => {
    mkdirSync(dirname(join(proj, p)), { recursive: true });
    writeFileSync(join(proj, p), c);
  };
  write("package.json", JSON.stringify({ name: "smoke", private: true, type: "module" }));
  write("src/index.ts", 'import "./a";\nimport "./b";\nimport "./c";\n');
  write("src/a.ts", 'import "./b";\n');
  write("src/b.ts", 'import "./a";\n');
  write("src/c.ts", "export {};\n");
  write(
    "oxcruise.config.ts",
    `import { defineConfig } from "oxcruise";\n(globalThis as any).__configPid = process.pid;\n\nexport default defineConfig({\n  entries: ["src"],\n  jsPlugins: ["./plugin.ts"],\n  rules: { "no-circular": "warn", "smoke/where": "info" },\n});\n`,
  );
  write(
    "plugin.ts",
    `import { definePlugin } from "oxcruise/plugin";\n\nexport default definePlugin({\n  meta: { name: "smoke" },\n  rules: {\n    where: {\n      create(ctx) {\n        return { module(m) { if (m.path === "src/index.ts") ctx.report({ module: m, message: "config-in-host=" + ((globalThis as any).__configPid === process.pid) + " deps=" + m.dependencies.length }); } };\n      },\n    },\n  },\n  reporters: { "json-lines": (r) => r.violations.map((v) => JSON.stringify({ rule: v.rule, from: v.from })).join("\\n") + "\\n" },\n});\n`,
  );
  run("npm", ["install", "--no-audit", "--no-fund", "--loglevel=error", ...tarballs], {
    cwd: proj,
  });
  check(
    "npm install of the tarballs",
    existsSync(join(proj, "node_modules/oxcruise/dist/cli.js")) &&
      existsSync(join(proj, `node_modules/@oxcruise/binding-${t}`)),
  );

  // 3. run it the way users do
  const plain = attempt("npx", ["--no-install", "oxcruise", "--color", "never"], { cwd: proj });
  check(
    "exits 0: only warnings and infos",
    plain.code === 0,
    `${plain.code}\n${plain.stdout}\n${plain.stderr}`,
  );
  check(
    "TS config loaded in the host process (not a node subprocess)",
    plain.stdout.includes("config-in-host=true"),
    plain.stdout,
  );
  check("TS plugin rule ran with the whole graph", plain.stdout.includes("deps=3"), plain.stdout);
  check("native rule still works alongside", plain.stdout.includes("no-circular"), plain.stdout);
  check(
    "no dependency-cruiser importer note for a native config",
    !plain.stderr.includes("importer"),
    plain.stderr,
  );

  const lines = attempt("npx", ["--no-install", "oxcruise", "-f", "smoke/json-lines"], {
    cwd: proj,
  });
  const rules = lines.stdout
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l).rule)
    .sort();
  check(
    "plugin reporter output",
    rules.join() === "no-circular,smoke/where",
    `${lines.code}\n${lines.stdout}\n${lines.stderr}`,
  );

  const help = attempt("npx", ["--no-install", "oxcruise", "--version"], { cwd: proj });
  check("--version", /oxcruise \d+\.\d+\.\d+/.test(help.stdout), help.stdout + help.stderr);
} finally {
  if (process.env.KEEP_SMOKE) console.log(`kept ${work}`);
  else rmSync(work, { recursive: true, force: true });
}
console.log(failures ? `\n${failures} check(s) failed` : "\nsmoke test passed");
process.exit(failures ? 1 : 0);
