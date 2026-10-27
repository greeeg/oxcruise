# oxcruise

Validate and visualise your JS/TS module graph. A [dependency-cruiser](https://github.com/sverweij/dependency-cruiser)
alternative built on the [oxc](https://oxc.rs) toolchain: one parser, parallel everywhere, a modern config, and
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

## Why

* **Fast.** Reading, parsing, extracting and resolving run in parallel on every core; cycles are found with one SCC pass
  for the whole graph. See [benchmarks](tasks/benchmark/README.md).
* **One parser, no peer dependencies.** TS, TSX, MTS, CTS, JSX, Vue, Svelte and Astro, with exact `type-only` information.
  No `typescript` install needed.
* **Monorepo-aware.** The nearest tsconfig per file, workspace packages, `#imports`.
* **Good defaults, few knobs.** Like oxlint: rules come in categories (`correctness` is on as `error`, `suspicious` as
  `warn`), globs are the only pattern language, and cycles that exist only through `import type` are not reported.
* **Rules that say what you mean.** Layers, `internal/` folders, `{name}` captures and `!` negation in globs
  (`"to": ["src/features/**", "!src/features/{feature}/**"]`), relation predicates (`descendant-of-from-parent`).
* **Plugins in JS/TS** over the finished graph, plus plugin reporters, plus a programmatic API for architecture tests.
* **Inline suppression** with `// oxcruise-ignore-next-line no-circular -- reason`.
* **A migration path.** `oxcruise migrate` converts your `.dependency-cruiser.*` and says what it could not carry over.

## Quick start

```sh
npm i -D oxcruise
npx oxcruise                       # your config, or the defaults
npx oxcruise src -f dot | dot -Tsvg > graph.svg
npx oxcruise -f github             # annotations in GitHub Actions
npx oxcruise --rules               # the built-in rules, as JSON
```

```ts
// oxcruise.config.ts
import { defineConfig } from "oxcruise";

export default defineConfig({
  entries: ["src"],
  ignorePatterns: ["src/generated/**"],           // .gitignore is always honoured
  categories: { correctness: "error", suspicious: "warn" },   // this is the default
  rules: {
    "no-circular": "error",
    layers: ["error", { layers: { ui: "src/ui/**", domain: "src/domain/**" }, allow: { ui: ["domain"], domain: [] } }],
  },
  forbid: [
    { name: "feature-isolation", message: "features must not import each other",
      from: "src/features/{feature}/**",
      to: ["src/features/**", "!src/features/{feature}/**"] },
    { name: "no-node-in-browser", from: "src/web/**", to: { kind: "builtin" } },
  ],
  overrides: [{ files: ["scripts/**"], rules: { "no-dev-deps-in-prod": "off" } }],
});
```

The config is one `oxcruise.config.ts` or `.oxcruiserc.json` (comments allowed), `$schema` for editors. See
[Configuration](docs/architecture.md#configuration) for every key.

A plugin rule and the programmatic API:

```ts
import { definePlugin } from "oxcruise/plugin";
export default definePlugin({
  meta: { name: "acme" },
  rules: { "max-fan-out": { create: (ctx) => ({ module(m) { /* ctx.report({ module: m, message }) */ } }) } },
});

// in a test
import { cruise } from "oxcruise";
const g = await cruise({ paths: ["src"] });
expect(g.module("src/ui/page.ts")?.dependsOn("src/db/**")).toBe(false);
```

## Documentation

* [Architecture and design](docs/architecture.md): the gaps it closes, how it works, the plugin API, and where the
  implementation differs from the original plan.
* [Rules](docs/rules.md)
* [Migrating from dependency-cruiser](docs/migrating-from-depcruise.md)
* [Conformance with dependency-cruiser 18.7](docs/conformance.md)

## Status

Early. The core, rules, reporters, CLI, JS plugins and npm packaging are implemented and tested on Linux/arm64. Nothing
is published, the release workflow has not run, and other platforms are untested. Not built yet: baseline, cache, LSP and
`--watch`, SARIF and other CI reporters, unused-export detection. [Details](docs/architecture.md#where-this-differs-from-the-original-plan).

## Development

```sh
just ready                 # fmt, clippy, cargo test, typecheck, vitest
just oxcruise-node-debug   # build the napi addon + bundle the JS (needed by the vitest e2e tests)
just smoke                 # npm pack, install the tarballs into a fresh project, run npx oxcruise
just conformance           # diff against dependency-cruiser 18.7
just bench
```

Requires Rust (stable), Node ≥ 22.18 and pnpm. `pnpm install` also installs the git hooks ([lefthook](https://lefthook.dev)):
`pre-commit` formats staged Rust (rustfmt) and JS/TS ([oxfmt](https://oxc.rs/docs/guide/usage/formatter)) files, `pre-push` runs
`just lint`. CI checks the same formatting, so `git commit --no-verify` only defers the failure.

Licensed MIT; see [NOTICE](NOTICE) for attributions.
