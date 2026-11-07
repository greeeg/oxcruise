<p align="left">
  <img src="https://github.com/greeeg/oxcruise/blob/master/.github/assets/logo.png?raw=true" alt="oxcruise" width="240" />
</p>

Validate and visualise your JS/TS module graph. A [dependency-cruiser](https://github.com/sverweij/dependency-cruiser)
alternative built on the [oxc](https://oxc.rs) toolchain.

```sh
npx oxcruise src
```

```
error[circular-dependencies]: circular dependency: src/a.ts → src/b.ts → src/a.ts
  --> src/a.ts:1:19
   |
 1 | import { b } from "./b";
   |                   ^^^^^
   = cycle:
     ╭─▶ src/a.ts
     ╰── src/b.ts

✖ 1 problem (1 error, 0 warnings) in 24 modules, 31 dependencies
```

It needs no config to start. The default rules catch circular imports, unresolvable imports, undeclared and dev-only
dependencies, and orphan modules. When you need more, you add layers, `forbid` rules and plugins.

## Why

- **Fast.** Reading, parsing, extracting and resolving run in parallel on every core, and cycles are found with one pass
  over the whole graph. See the [benchmarks](https://github.com/greeeg/oxcruise/blob/master/tasks/benchmark/README.md).
- **One parser, no peer dependencies.** TS, TSX, MTS, CTS, JSX, Vue, Svelte and Astro, with exact `type-only` information.
  You do not install `typescript`.
- **Monorepo-aware.** The nearest `tsconfig.json` per file, workspace packages and `#imports`.
- **Good defaults, few knobs.** Like oxlint, rules come in categories (`correctness` is on as `error`, `suspicious` as
  `warn`), globs are the only pattern language, and cycles that exist only through `import type` are not reported.
- **Rules that say what you mean.** Layers, folder visibility, folder structure, `{name}` captures and `!` negation in globs.
- **Plugins in TypeScript** that see the finished graph, plugin reporters, and a programmatic API for architecture tests.
- **Unused code.** Unused exports, dependencies and files, modelled on [Knip](https://knip.dev).
- **A migration path.** `oxcruise --migrate` converts your `.dependency-cruiser.*` file and says what it could not carry over.

## Quick start

Oxcruise needs Node.js 22.18 or later.

```sh
npm install --save-dev oxcruise
npx oxcruise                       # check the current folder with the default rules
npx oxcruise src                   # start at src instead
npx oxcruise --init                # write a starter oxcruise.config.ts
```

The command exits with `0` when there are no errors, `1` when there are errors, and `2` for a bad flag or config file.

Other things to try:

```sh
npx oxcruise --visualize                          # an interactive architecture site in ./oxcruise-report
npx oxcruise -f github                            # annotations in GitHub Actions
npx oxcruise --rules                              # the built-in rules by category
npx oxcruise --print-config                       # the config after defaults and `extends`
```

Add `--deny-warnings` or `--max-warnings <N>` to fail a CI run on warnings.

## Configure

Write the config in `oxcruise.config.ts`, or in `.oxcruiserc.json` (comments allowed, and a `$schema` for editors):

```ts
// oxcruise.config.ts
import { defineConfig } from "oxcruise";

export default defineConfig({
  entries: ["src"],
  ignorePatterns: ["src/generated/**"], // .gitignore is always honoured
  categories: { correctness: "error", suspicious: "warn" }, // this is the default
  rules: {
    "circular-dependencies": "error",
    layers: {
      level: "error",
      layers: { ui: "src/ui/**", domain: "src/domain/**" },
      allow: { ui: ["domain"], domain: [] },
    },
  },
  forbid: [
    {
      name: "feature-isolation",
      message: "features must not import each other",
      from: "src/features/{feature}/**",
      to: ["src/features/**", "!src/features/{feature}/**"],
    },
    { name: "no-node-in-browser", from: "src/web/**", to: { kind: "builtin" } },
  ],
  overrides: [{ files: ["scripts/**"], rules: { "dev-dependencies-in-production": "off" } }],
});
```

To silence one violation, say why in a comment:

```ts
// oxcruise-ignore-next-line circular-dependencies -- the registry needs the plugin list
import { plugins } from "./plugins";
```

See [Configuration](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/guide/configuration.mdx) and the [config reference](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/reference/config.mdx)
for every key.

## Built-in rules

Run `npx oxcruise --rules` for the list on your version.

| Category      | Default | Rules                                                                                                                         |
| ------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `correctness` | error   | `circular-dependencies`, `unresolved-dependencies`, `undeclared-packages`, `dev-dependencies-in-production`, `dependencies-on-tests`, `cross-package-relative-dependencies` |
| `suspicious`  | warn    | `orphan-modules`, `duplicate-packages`, `deprecated-dependencies`, `unused-exports`, `unused-packages`, `unused-modules`                                                                            |
| `style`       | off     | `folder-structure`                                                                                                            |
| `restriction` | off     | `layers`, `visibility`, `optional-dependencies`, `peer-dependencies`                                                              |

The unused-code rules need exports, so they cost an extra walk. They are on by default. Set them to `off` in `rules` to skip it, add `--production` to leave
tests out, and `--fix` to apply the automatic repairs.
See [Rules and categories](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/features/rules-and-categories.mdx) and the [rule reference](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/reference/rules/index.mdx).

## Plugins and the API

A plugin rule runs over the finished graph:

```ts
import { definePlugin } from "oxcruise/plugin";

export default definePlugin({
  meta: { name: "acme" },
  rules: {
    "max-fan-out": {
      create: (ctx) => ({
        module(m) {
          /* ctx.report({ module: m, message }) */
        },
      }),
    },
  },
});
```

The same graph is available from code, for architecture tests:

```ts
import { cruise } from "oxcruise";

const g = await cruise({ paths: ["src"] });
expect(g.module("src/ui/page.ts")?.dependsOn("src/db/**")).toBe(false);
```

See [JS plugins](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/features/js-plugins.mdx) and the [programmatic API](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/features/programmatic-api.mdx).

## Coming from dependency-cruiser

Run `npx oxcruise --migrate`. It reads your `.dependency-cruiser.*` file, writes `oxcruise.config.ts`, and lists what it could
not convert. [Migrate from dependency-cruiser](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/guide/migrate-from-dependency-cruiser.mdx) explains the mapping and lists
where Oxcruise behaves differently (it is tested against dependency-cruiser 18.7).

## Documentation

The documentation site is in [`website/`](https://github.com/greeeg/oxcruise/tree/master/website) and is built with [Vocs](https://vocs.dev). Until it has its own URL, read the pages
on GitHub:

- Guide: [Quickstart](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/guide/quickstart.mdx), [Configuration](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/guide/configuration.mdx),
  [Monorepos](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/guide/monorepos.mdx)
- Features: [Output formats](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/features/output-formats.mdx),
  [Ignore code](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/features/ignore.mdx)
- Reference: [CLI](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/reference/cli.mdx), [config](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/reference/config.mdx),
  [rules](https://github.com/greeeg/oxcruise/blob/master/website/src/pages/reference/rules/index.mdx)

