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
npx oxcruise --init                # write a starter oxcruise.config.ts
npx oxcruise                       # your config, or the defaults
npx oxcruise src -f dot | dot -Tsvg > graph.svg
npx oxcruise -f github             # annotations in GitHub Actions
npx oxcruise --rules               # the built-in rules by category (-f json for JSON)
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
[Configuration](website/src/pages/guide/configuration.mdx) and the [config reference](website/src/pages/reference/config.mdx)
for every key.

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

// exports and who uses them (extracts symbols, so it costs an extra walk; a plugin asks for them
// with `definePlugin({ needs: ["symbols"], … })`, the CLI with `--symbols` or `"symbols": true`)
const s = await cruise({ paths: ["src"], symbols: true });
expect(s.module("src/index.ts")!.unusedExports()).toEqual([]);
expect(s.whoImports("src/db/client.ts", "query").every((m) => m.path.startsWith("src/repo/"))).toBe(true);
```

Unused exports, dependencies and files (modelled on [Knip](https://knip.dev)) are the nursery rules
`no-unused-exports`, `no-unused-deps` and `no-unused-files`: enable them in `rules`, add `--production` (`cruise({ production: true })`) to leave tests
out, and `--fix` to apply the automatic repairs.

## Documentation

The documentation site is in [`website/`](website) (built with [Vocs](https://vocs.dev); see [`website/README.md`](website/README.md)).
Until it has a URL, read the pages in `website/src/pages`:

* Guide: [Quickstart](website/src/pages/guide/quickstart.mdx), [Configuration](website/src/pages/guide/configuration.mdx),
  [Monorepos](website/src/pages/guide/monorepos.mdx), [CI](website/src/pages/guide/ci.mdx),
  [Coding agents](website/src/pages/guide/coding-agents.mdx)
* Features: [Rules and categories](website/src/pages/features/rules-and-categories.mdx),
  [Forbid rules](website/src/pages/features/forbid-rules.mdx), [Unused code](website/src/pages/features/unused-code.mdx),
  [Output formats](website/src/pages/features/output-formats.mdx), [JS plugins](website/src/pages/features/js-plugins.mdx),
  [Programmatic API](website/src/pages/features/programmatic-api.mdx)
* Migration: [From dependency-cruiser](website/src/pages/migration/dependency-cruiser.mdx),
  [Differences (conformance with 18.7)](website/src/pages/migration/differences.mdx)
* Reference: [CLI](website/src/pages/reference/cli.mdx), [config](website/src/pages/reference/config.mdx),
  [rules](website/src/pages/reference/rules/index.mdx)
* For contributors: [Architecture and design](docs/architecture.md), the gaps it closes, how it works, and where the
  implementation differs from the original plan.

## Status

Early. The core, rules (including the nursery rules `no-unused-exports`, `no-unused-deps` and `no-unused-files`), reporters,
CLI, JS plugins and npm packaging are implemented and tested on Linux/arm64. Nothing is published, the release workflow has
not run, and other platforms are untested. Not built yet: baseline, cache, LSP and `--watch`, SARIF and other CI reporters.
[Details](docs/architecture.md#where-this-differs-from-the-original-plan).

## Development

```sh
just ready                 # fmt, clippy, cargo test, typecheck, vitest
just oxcruise-node-debug   # build the napi addon + bundle the JS (needed by the vitest e2e tests)
just smoke                 # npm pack, install the tarballs into a fresh project, run npx oxcruise
just conformance           # diff against dependency-cruiser 18.7
just bench
just website-dev           # the documentation site (website/); `just website-gen` regenerates its reference pages
```

Requires Rust (stable), Node ≥ 22.18 and pnpm. `pnpm install` also installs the git hooks ([lefthook](https://lefthook.dev)):
`pre-commit` formats staged Rust (rustfmt) and JS/TS ([oxfmt](https://oxc.rs/docs/guide/usage/formatter)) files. CI checks the same formatting, so `git commit --no-verify` only defers the failure.

Licensed MIT; see [NOTICE](NOTICE) for attributions.
