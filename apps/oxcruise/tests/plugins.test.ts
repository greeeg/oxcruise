import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { beforeEach, describe, expect, test } from "vitest";
import {
  loadPlugin,
  packageCandidates,
  PluginRegistry,
  resolveSpecifier,
  validatePlugin,
} from "../src-js/plugins/load.ts";
import { withDefaults } from "../src-js/plugins/run.ts";

let dir: string;
let registry: PluginRegistry;
beforeEach(() => {
  dir = mkdtempSync(join(tmpdir(), "oxcruise-plugins-"));
  registry = new PluginRegistry();
});

const write = (path: string, content: string) => {
  const full = join(dir, path);
  mkdirSync(join(full, ".."), { recursive: true });
  writeFileSync(full, content);
  return full;
};
const plugin = (name: string, extra = "") =>
  `export default { meta: { name: "${name}" }, rules: { r: { create() { return {}; } } }, reporters: { rep: () => "x" }${extra} };\n`;

describe("specifier resolution", () => {
  test("relative and absolute paths", () => {
    expect(resolveSpecifier("./p.mjs", dir)).toBe(join(dir, "p.mjs"));
    expect(resolveSpecifier(join(dir, "q.mjs"), "/elsewhere")).toBe(join(dir, "q.mjs"));
  });

  test("package names try the oxcruise-plugin- prefix first", () => {
    expect(packageCandidates("acme")).toEqual(["oxcruise-plugin-acme", "acme"]);
    expect(packageCandidates("oxcruise-plugin-acme")).toEqual(["oxcruise-plugin-acme"]);
    expect(packageCandidates("@acme/lint")).toEqual(["@acme/oxcruise-plugin-lint", "@acme/lint"]);
    expect(packageCandidates("@acme/oxcruise-plugin-lint")).toEqual(["@acme/oxcruise-plugin-lint"]);

    write(
      "node_modules/oxcruise-plugin-prefixed/package.json",
      '{"name":"oxcruise-plugin-prefixed","main":"index.cjs"}',
    );
    write("node_modules/oxcruise-plugin-prefixed/index.cjs", "module.exports = {};");
    write("node_modules/plain/package.json", '{"name":"plain","main":"index.cjs"}');
    write("node_modules/plain/index.cjs", "module.exports = {};");
    expect(resolveSpecifier("prefixed", dir)).toContain("oxcruise-plugin-prefixed");
    expect(resolveSpecifier("plain", dir)).toContain(join("node_modules", "plain"));
  });

  test("a missing package lists what was tried", () => {
    expect(() => resolveSpecifier("ghost", dir)).toThrow(
      /cannot find plugin `ghost`.*oxcruise-plugin-ghost, ghost/,
    );
  });
});

describe("loading", () => {
  test("loads a plugin by path and reports its qualified names", async () => {
    write("a.mjs", plugin("acme"));
    const meta = await loadPlugin({ specifier: "./a.mjs", cwd: dir }, registry);
    expect(meta).toEqual({ name: "acme", rules: ["r"], reporters: ["rep"] });
    expect(registry.rule("acme/r").rule.create).toBeTypeOf("function");
    expect(registry.reporter("acme/rep")({} as never)).toBe("x");
  });

  test("loads a plugin from a package", async () => {
    write(
      "node_modules/oxcruise-plugin-pkg/package.json",
      '{"name":"oxcruise-plugin-pkg","type":"module","exports":{".":{"import":"./i.js","require":"./i.js"}}}',
    );
    write("node_modules/oxcruise-plugin-pkg/i.js", plugin("pkg"));
    const meta = await loadPlugin({ specifier: "pkg", cwd: dir }, registry);
    expect(meta.name).toBe("pkg");
  });

  test("an alias replaces meta.name", async () => {
    write("a.mjs", plugin("acme"));
    const meta = await loadPlugin({ specifier: "./a.mjs", alias: "short", cwd: dir }, registry);
    expect(meta.name).toBe("short");
    expect(() => registry.rule("short/r")).not.toThrow();
    expect(() => registry.rule("acme/r")).toThrow(/no loaded plugin provides the rule/);
  });

  test("two different plugins with one name collide; the same plugin twice does not", async () => {
    write("a.mjs", plugin("acme"));
    write("b.mjs", plugin("acme"));
    await loadPlugin({ specifier: "./a.mjs", cwd: dir }, registry);
    await loadPlugin({ specifier: "./a.mjs", cwd: dir }, registry);
    await expect(loadPlugin({ specifier: "./b.mjs", cwd: dir }, registry)).rejects.toThrow(
      /two plugins are named `acme`.*alias/,
    );
    await expect(
      loadPlugin({ specifier: "./b.mjs", alias: "other", cwd: dir }, registry),
    ).resolves.toMatchObject({ name: "other" });
  });

  test("TypeScript plugins load through native type stripping", async () => {
    write("t.ts", `const n: string = "ts";\nexport default { meta: { name: n }, rules: {} };\n`);
    await expect(loadPlugin({ specifier: "./t.ts", cwd: dir }, registry)).resolves.toMatchObject({
      name: "ts",
    });
  });

  test("an import failure names the file", async () => {
    write("broken.mjs", "throw new Error('boom');");
    await expect(loadPlugin({ specifier: "./broken.mjs", cwd: dir }, registry)).rejects.toThrow(
      /failed to import .*broken\.mjs: boom/,
    );
  });
});

describe("validation of bad plugin shapes", () => {
  const bad = (value: unknown) => () => validatePlugin(value, "x.ts");

  test("not an object", () => {
    expect(bad(null)).toThrow(/default export must be an object/);
    expect(bad("nope")).toThrow(/default export must be an object/);
  });
  test("meta.name", () => {
    expect(bad({})).toThrow(/`meta.name` must be a lowercase string/);
    expect(bad({ meta: { name: "Has Spaces" } })).toThrow(/`meta.name`/);
  });
  test("unknown properties", () => {
    expect(bad({ meta: { name: "a" }, rulez: {} })).toThrow(/unknown property `rulez`/);
  });
  test("rules need create()", () => {
    expect(bad({ meta: { name: "a" }, rules: { r: {} } })).toThrow(
      /rule `r` must have a `create\(context\)` function/,
    );
    expect(bad({ meta: { name: "a" }, rules: { "Bad Name": { create() {} } } })).toThrow(
      /invalid rule name/,
    );
  });
  test("reporters are functions", () => {
    expect(bad({ meta: { name: "a" }, reporters: { r: "no" } })).toThrow(
      /reporter `r` must be a function/,
    );
  });
  test("a valid plugin passes through", () => {
    const p = {
      meta: { name: "a" },
      rules: {
        r: {
          create() {
            return {};
          },
        },
      },
    };
    expect(validatePlugin(p, "x.ts")).toBe(p);
  });
});

describe("rule options", () => {
  const schema = {
    max: { type: "number", default: 15 },
    name: { default: "x" },
    plain: { type: "string" },
  };
  test("defaults fill the gaps and configured values win", () => {
    expect(withDefaults(schema, undefined)).toEqual({ max: 15, name: "x" });
    expect(withDefaults(schema, null)).toEqual({ max: 15, name: "x" });
    expect(withDefaults(schema, { max: 3, extra: true })).toEqual({
      max: 3,
      name: "x",
      extra: true,
    });
    expect(withDefaults(undefined, { a: 1 })).toEqual({ a: 1 });
    expect(withDefaults(schema, ["not", "an", "object"])).toEqual({ max: 15, name: "x" });
  });
});
