# oxcruise

Validate and visualise your JS/TS module graph. A [dependency-cruiser](https://github.com/sverweij/dependency-cruiser)
alternative built on the [oxc](https://oxc.rs) toolchain: one parser, parallel everywhere, a small config, and
rules you can write in TypeScript.

```sh
npx oxcruise src
```

```
error[no-circular]: circular dependency: src/a.ts → src/b.ts → src/a.ts
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
  over the whole graph. See the [benchmarks](tasks/benchmark/README.md).
- **One parser, no peer dependencies.** TS, TSX, MTS, CTS, JSX, Vue, Svelte and Astro, with exact `type-only` information.
  You do not install `typescript`.
- **Monorepo-aware.** The nearest `tsconfig.json` per file, workspace packages and `#imports`.
- **Good defaults, few knobs.** Like oxlint, rules come in categories (`correctness` is on as `error`, `suspicious` as
  `warn`), globs are the only pattern language, and cycles that exist only through `import type` are not reported.
- **Rules that say what you mean.** Layers, `internal/` folders, `{name}` captures and `!` negation in globs.
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
npx oxcruise src -f dot | dot -Tsvg > graph.svg   # a picture of the graph (Graphviz)
npx oxcruise -f mermaid                           # the same as a Mermaid diagram
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
    "no-circular": "error",
    layers: [
      "error",
      {
        layers: { ui: "src/ui/**", domain: "src/domain/**" },
        allow: { ui: ["domain"], domain: [] },
      },
    ],
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
  overrides: [{ files: ["scripts/**"], rules: { "no-dev-deps-in-prod": "off" } }],
});
```

To silence one violation, say why in a comment:

```ts
// oxcruise-ignore-next-line no-circular -- the registry needs the plugin list
import { plugins } from "./plugins";
```

See [Configuration](website/src/pages/guide/configuration.mdx) and the [config reference](website/src/pages/reference/config.mdx)
for every key.

## Built-in rules

Run `npx oxcruise --rules` for the list on your version.

| Category      | Default | Rules                                                                                                                         |
| ------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------- |
| `correctness` | error   | `no-circular`, `no-unresolvable`, `no-undeclared-deps`, `no-dev-deps-in-prod`, `no-test-imports`, `no-cross-package-relative` |
| `suspicious`  | warn    | `no-orphans`, `no-duplicate-deps`, `no-deprecated`                                                                            |
| `restriction` | off     | `layers`, `internal-modules`, `no-optional-deps`, `no-peer-deps`                                                              |
| `nursery`     | off     | `no-unused-exports`, `no-unused-deps`, `no-unused-files`                                                                      |

The unused-code rules need exports, so they cost an extra walk. Enable them in `rules`, add `--production` to leave
tests out, and `--fix` to apply the automatic repairs.
See [Rules and categories](website/src/pages/features/rules-and-categories.mdx) and the [rule reference](website/src/pages/reference/rules/index.mdx).

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

See [JS plugins](website/src/pages/features/js-plugins.mdx) and the [programmatic API](website/src/pages/features/programmatic-api.mdx).

## Coming from dependency-cruiser

Run `npx oxcruise --migrate`. It reads your `.dependency-cruiser.*` file, writes `oxcruise.config.ts`, and lists what it could
not convert. [Migrate from dependency-cruiser](website/src/pages/migration/dependency-cruiser.mdx) explains the mapping, and
[Differences](website/src/pages/migration/differences.mdx) lists where Oxcruise behaves differently (it is tested against
dependency-cruiser 18.7).

## Documentation

The documentation site is in [`website/`](website) and is built with [Vocs](https://vocs.dev). Until it has its own URL, read the pages
on GitHub:

- Guide: [Quickstart](website/src/pages/guide/quickstart.mdx), [Configuration](website/src/pages/guide/configuration.mdx),
  [Monorepos](website/src/pages/guide/monorepos.mdx), [CI](website/src/pages/guide/ci.mdx),
  [Coding agents](website/src/pages/guide/coding-agents.mdx)
- Features: [Forbid rules](website/src/pages/features/forbid-rules.mdx),
  [Layers and internal modules](website/src/pages/features/layers-and-internal-modules.mdx),
  [Unused code](website/src/pages/features/unused-code.mdx), [Output formats](website/src/pages/features/output-formats.mdx),
  [Inline ignore comments](website/src/pages/features/inline-ignore-comments.mdx)
- Reference: [CLI](website/src/pages/reference/cli.mdx), [config](website/src/pages/reference/config.mdx),
  [rules](website/src/pages/reference/rules/index.mdx)

<!-- repo-only:start -->

## Development

```sh
just ready                 # fmt, clippy, cargo test, typecheck, vitest
just oxcruise-node-debug   # build the napi addon + bundle the JS (needed by the vitest e2e tests)
just smoke                 # npm pack, install the tarballs into a fresh project, run npx oxcruise
just conformance           # diff against dependency-cruiser 18.7
just bench
just website-dev           # the documentation site (website/); `just website-gen` regenerates its reference pages
just sync-readme           # copy this README and LICENSE into npm/oxcruise/ (the pre-commit hook does it too)
```

Requires Rust (stable), Node ≥ 22.18 and pnpm. `pnpm install` also installs the git hooks ([lefthook](https://lefthook.dev)):
`pre-commit` formats staged Rust (rustfmt) and JS/TS ([oxfmt](https://oxc.rs/docs/guide/usage/formatter)) files. CI checks the same formatting, so `git commit --no-verify` only defers the failure.
For the design, the gaps it closes and where the implementation differs from the original plan, read [Architecture and design](docs/architecture.md).

The npm package gets a copy of this README with its relative links rewritten to GitHub URLs (`tasks/npm/sync-readme.mjs`, run by the
pre-commit hook and checked in CI). Do not edit `npm/oxcruise/README.md`.
Keep links relative, and put anything meant for contributors between the `repo-only` comments.

<!-- repo-only:end -->
