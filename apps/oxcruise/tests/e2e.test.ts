import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { describe, expect, test } from "vitest";
import { cruise } from "../src-js/cruise.ts";
import { fixtures, project, root, runCli } from "./helpers.ts";

const pluginProject = join(fixtures, "plugin-project");
const defineUrl = join(root, "src-js/package/define.ts");

describe("plugin rules through the CLI", () => {
  test("module, dependency and options-driven rules report with locations", async () => {
    const out = await runCli(pluginProject, []);
    expect(out.code).toBe(1);
    expect(out.stderr).toBe("");
    expect(out.stdout).toMatchSnapshot();
  });

  test("rule options come from the config; defaults from meta.schema fill the rest", async () => {
    // max defaults to 15 when no options are given: nothing exceeds it
    const dir = project({
      "src/index.ts": 'import "./a"; import "./b";\n',
      "src/a.ts": "export {};\n",
      "src/b.ts": "export {};\n",
      ".oxcruiserc.json": JSON.stringify({
        entries: ["src"],
        jsPlugins: [join(fixtures, "plugins/acme.ts")],
        rules: { "acme/max-fan-out": "warn" },
      }),
    });
    expect((await runCli(dir, [])).code).toBe(0);
    const strict = project({
      "src/index.ts": 'import "./a"; import "./b";\n',
      "src/a.ts": "export {};\n",
      "src/b.ts": "export {};\n",
      ".oxcruiserc.json": JSON.stringify({
        entries: ["src"],
        jsPlugins: [join(fixtures, "plugins/acme.ts")],
        rules: { "acme/max-fan-out": ["error", { max: 1 }] },
      }),
    });
    const o = await runCli(strict, []);
    expect(o.code).toBe(1);
    expect(o.stdout).toContain("fan-out 2 is greater than 1");
  });

  test("a graph-level rule with before/graph/after hooks sees the cycles", async () => {
    const dir = project({
      "src/a.ts": 'import "./b";\n',
      "src/b.ts": 'import "./a";\n',
      ".oxcruiserc.json": JSON.stringify({
        entries: ["src"],
        jsPlugins: [join(fixtures, "plugins/acme.ts")],
        rules: { "acme/cycle-budget": ["error", { max: 0 }] },
      }),
    });
    const o = await runCli(dir, []);
    expect(o.code).toBe(1);
    expect(o.stdout).toContain("1 cycle(s) exceed the budget of 0");
  });

  test("inline suppression comments apply to plugin rules, edge-level and file-level", async () => {
    // the fixture already suppresses one edge; assert it did not leak into the report
    const o = await runCli(pluginProject, ["-f", "json"]);
    const result = JSON.parse(o.stdout);
    const direct = result.violations.filter(
      (v: { rule: string }) => v.rule === "acme/no-direct-db",
    );
    expect(direct).toHaveLength(1);
    expect(direct[0].message).toContain("src/db/query.ts");
    expect(result.summary.suppressed).toBe(1);

    const file = project({
      "src/index.ts": "// oxcruise-ignore-file acme/max-fan-out\nimport './a'; import './b';\n",
      "src/a.ts": "export {};\n",
      "src/b.ts": "export {};\n",
      ".oxcruiserc.json": JSON.stringify({
        entries: ["src"],
        jsPlugins: [join(fixtures, "plugins/acme.ts")],
        rules: { "acme/max-fan-out": ["error", { max: 1 }] },
      }),
    });
    expect((await runCli(file, [])).code).toBe(0);
  });

  test("a plugin that throws fails the run instead of passing silently", async () => {
    const dir = project({
      "src/a.ts": "export {};\n",
      "plugin.mjs":
        'export default { meta: { name: "boom" }, rules: { explode: { create() { return { module() { throw new Error("kaboom"); } }; } } } };\n',
      ".oxcruiserc.json": JSON.stringify({
        entries: ["src"],
        jsPlugins: ["./plugin.mjs"],
        rules: { "boom/explode": "error" },
      }),
    });
    const o = await runCli(dir, []);
    expect(o.code).toBe(2);
    expect(o.stderr).toContain("plugin rule `boom/explode` threw: kaboom");
  });

  test("configuring a rule the plugin does not have is a clear error", async () => {
    const dir = project({
      "src/a.ts": "export {};\n",
      ".oxcruiserc.json": JSON.stringify({
        entries: ["src"],
        jsPlugins: [join(fixtures, "plugins/acme.ts")],
        rules: { "acme/nope": "error" },
      }),
    });
    const o = await runCli(dir, []);
    expect(o.code).toBe(2);
    expect(o.stderr).toMatch(/unknown rule `acme\/nope`/);
  });

  test("an alias in jsPlugins renames the rules", async () => {
    const dir = project({
      "src/index.ts": "import './a'; import './b';\n",
      "src/a.ts": "export {};\n",
      "src/b.ts": "export {};\n",
      ".oxcruiserc.json": JSON.stringify({
        entries: ["src"],
        jsPlugins: [{ name: "team", specifier: join(fixtures, "plugins/acme.ts") }],
        rules: { "team/max-fan-out": ["error", { max: 1 }] },
      }),
    });
    const o = await runCli(dir, []);
    expect(o.code).toBe(1);
    expect(o.stdout).toContain("team/max-fan-out");
  });
});

describe("plugin reporters", () => {
  test("-f plugin/reporter renders through the plugin", async () => {
    const o = await runCli(pluginProject, ["-f", "acme/json-lines"]);
    expect(o.code).toBe(1);
    const lines = o.stdout
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l));
    expect(lines.map((l) => l.rule).sort()).toEqual(["acme/max-fan-out", "acme/no-direct-db"]);
    expect(lines.find((l) => l.rule === "acme/no-direct-db")).toMatchObject({
      severity: "error",
      file: "src/ui/page.ts",
      line: 3,
    });
  });

  test("an unknown reporter says which plugin should provide it", async () => {
    const o = await runCli(pluginProject, ["-f", "acme/nope"]);
    expect(o.code).toBe(2);
    expect(o.stderr).toMatch(/no loaded plugin provides the reporter `acme\/nope`/);
  });
});

describe("configs evaluated by the host Node", () => {
  test("a TypeScript config using defineConfig", async () => {
    const dir = project({
      "src/a.ts": "import './b';\n",
      "src/b.ts": "import './a';\n",
      "oxcruise.config.ts": `import { defineConfig } from ${JSON.stringify(defineUrl)};\n\nexport default defineConfig({\n  entries: ["src"],\n  rules: { "no-circular": "warn" },\n});\n`,
    });
    const o = await runCli(dir, []);
    expect(o.stderr).toBe("");
    expect(o.code).toBe(0);
    expect(o.stdout).toContain("warn[no-circular]");
  });

  test("a function config and an async one", async () => {
    const dir = project({
      "src/a.ts": "import './a';\n",
      "oxcruise.config.ts":
        'export default async () => ({ entries: ["src"], rules: { "no-circular": "error" } });\n',
    });
    expect((await runCli(dir, [])).code).toBe(1);
  });

  test("a config that throws names the file", async () => {
    const dir = project({
      "oxcruise.config.ts": "throw new Error('bad config');\n",
      "src/a.ts": "",
    });
    const o = await runCli(dir, []);
    expect(o.code).toBe(2);
    expect(o.stderr).toMatch(/cannot evaluate .*oxcruise\.config\.ts: bad config/);
  });

  test("a dependency-cruiser config is only read by `migrate`", async () => {
    const dir = project({
      "src/a.ts": "export {};\n",
      ".dependency-cruiser.cjs":
        "module.exports = { forbidden: [{ name: 'no-lib', severity: 'error', from: { path: '^src' }, to: { path: '^lib' } }] };\n",
    });
    // a check never looks at it ...
    const check = await runCli(dir, ["src"]);
    expect(check.code).toBe(0);
    expect(check.stderr).toBe("");
    // ... and refuses it when asked for explicitly
    const explicit = await runCli(dir, ["src", "-c", ".dependency-cruiser.cjs"]);
    expect(explicit.code).toBe(2);
    expect(explicit.stderr).toContain("unknown field");
    // `migrate` evaluates the CJS file in the host Node and writes the new shape
    const migrate = await runCli(dir, ["migrate"]);
    expect(migrate.stderr).toBe("");
    expect(migrate.code).toBe(0);
    const written = readFileSync(join(dir, "oxcruise.config.ts"), "utf8");
    expect(written).toContain('"name": "no-lib"');
    expect(written).toContain('"from": "src*"');
    expect(written).toContain('"to": "lib*"');
  });
});

describe("cruise() programmatic API", () => {
  test("answers architecture questions about a result", async () => {
    const g = await cruise({
      cwd: join(fixtures, "plugin-project"),
      noConfig: true,
      paths: ["src"],
    });
    expect(g.summary.total_modules).toBeGreaterThan(5);

    const page = g.module("src/ui/page.ts")!;
    expect(page.dependsOn("src/db/**")).toBe(true);
    expect(page.dependsOn("src/db/query.ts")).toBe(true);
    expect(page.dependsOn("src/ui/**")).toBe(false);
    expect(g.module("src/index.ts")!.dependsOn("src/db/**")).toBe(false);
    expect(g.module("src/index.ts")!.dependsOnTransitively("src/db/**")).toBe(false);
    expect(g.module("src/missing.ts")).toBeUndefined();

    expect(g.modulesMatching("src/db/*.ts").map((m) => m.path)).toEqual([
      "src/db/allowed.ts",
      "src/db/query.ts",
    ]);
    expect(g.module("src/db/query.ts")!.dependents.map((m) => m.path)).toEqual(["src/ui/page.ts"]);
  });

  test("module handles are built once and share the plugin graph", async () => {
    const g = await cruise({
      cwd: join(fixtures, "plugin-project"),
      noConfig: true,
      paths: ["src"],
    });
    expect(g.modules).toBe(g.modules);
    const page = g.module("src/ui/page.ts")!;
    expect(g.module("src/ui/page.ts")).toBe(page);
    expect(g.modules).toContain(page);
    expect(page.module).toBe(g.graph.module("src/ui/page.ts"));
    expect(g.modules.every((m) => m.kind !== "unresolved")).toBe(true);
  });

  test("transitive dependencies follow the graph", async () => {
    const dir = project({
      "src/ui.ts": "import './domain';\n",
      "src/domain.ts": "import './db';\n",
      "src/db.ts": "export {};\n",
    });
    const g = await cruise({ cwd: dir, noConfig: true, paths: ["src"] });
    expect(g.module("src/ui.ts")!.dependsOn("src/db.ts")).toBe(false);
    expect(g.module("src/ui.ts")!.dependsOnTransitively("src/db.ts")).toBe(true);
  });

  test("a result with errors is still a result; a failing run throws", async () => {
    const dir = project({ "src/a.ts": "import './a';\n" });
    const g = await cruise({ cwd: dir, noConfig: true, paths: ["src"] });
    expect(g.violations.map((v) => v.rule)).toContain("no-circular");
    expect(g.module("src/a.ts")!.violations).toHaveLength(1);
    await expect(cruise({ cwd: dir, config: "nope.json" })).rejects.toThrow(/does not exist/);
  });

  test("plugins run through cruise() too", async () => {
    const g = await cruise({ cwd: pluginProject });
    expect(g.violations.map((v) => v.rule).sort()).toEqual([
      "acme/max-fan-out",
      "acme/no-direct-db",
    ]);
  });
});

describe("misc", () => {
  test("the host never loads the plugin runtime for runs without plugins", async () => {
    const dir = project({
      "src/a.ts": "export {};\n",
      ".oxcruiserc.json": '{ "entries": ["src"] }',
    });
    expect((await runCli(dir, [])).code).toBe(0);
  });

  test("paths in output are relative to the cwd", async () => {
    const o = await runCli(pluginProject, ["-f", "text"]);
    expect(o.stdout).not.toContain(dirname(pluginProject));
  });
});
