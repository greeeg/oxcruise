# Contributing to oxcruise

Thanks for helping. This page covers how the code is organised ([Architecture](#architecture)) and how to build, test and
submit a change. The rules for performance and API consistency are in [AGENTS.md](AGENTS.md); they apply to people as
much as to coding agents.

## Prerequisites

- Rust: the stable toolchain (`rust-toolchain.toml` installs `clippy` and `rustfmt`).
- Node.js 22.18 or later (CI uses 24) and [pnpm](https://pnpm.io).
- [just](https://github.com/casey/just) to run the recipes in the `justfile`.

```sh
git clone https://github.com/greeeg/oxcruise
cd oxcruise
just install        # pnpm install; also installs the git hooks (lefthook)
just --list         # every recipe
```

## Architecture

oxcruise validates and visualises JS/TS module graphs, like [dependency-cruiser](https://github.com/sverweij/dependency-cruiser),
on the oxc toolchain: one parser, parallel everywhere, a modern config, and JS/TS plugins.

### Repository layout

| Path | What lives there |
|---|---|
| `crates/oxcruise_core` | Data model: `Graph` (forward + reverse CSR, deterministic order), `Module`, `Edge`, `DepKinds` bitflags with depcruise's names, `Violation`, `Severity`, `LineIndex`. No I/O. |
| `crates/oxcruise_extract` | One file → `FileFacts` (dependencies, suppression comments, export/import names). oxc parser + `ModuleRecord` + an AST visitor; SFC script-block splitter. |
| `crates/oxcruise_resolve` | One shared `oxc_resolver` per module system (ESM `import` condition, CJS `require`), dependency classification, package.json section lookup, workspace discovery. |
| `crates/oxcruise_graph` | The parallel runtime (discovery), path matcher, and analyses: Tarjan SCC, shortest cycle per edge, reachability, metrics, bitset. |
| `crates/oxcruise_config` | `OxcruiseConfig` (serde + schemars, `deny_unknown_fields`), JSONC, `extends` (file paths), node-subprocess loader behind `JsConfigLoader`, the `migrate` importer for dependency-cruiser configs. |
| `crates/oxcruise_macros` | `declare_rule!`: name, category, version, options type, docs from doc comments. |
| `crates/oxcruise_rules` | `Rule` trait, built-in rules, registry, the declarative compiler (`forbid`/`allow`/`require`), the engine (overrides, suppression, ordering, dead rules), the napi-free `ExternalRules` trait. |
| `crates/oxcruise_report` | `CruiseResult` model and the reporters. |
| `crates/oxcruise_visualize` | The compact data and the prebuilt site (`apps/visualizer`) of `--visualize`. |
| `apps/oxcruise` | One crate that is both a binary and a `cdylib`. `CliRunner`, `CliRunResult`, and with the `napi` feature the addon entry points and the plugin host. `src-js/` is the Node side. |
| `npm/oxcruise` | The published package; `npm/<triple>` are the `@oxcruise/binding-*` platform packages. |
| `tasks/` | `conformance` (diff against dependency-cruiser), `benchmark`, `smoke` (npm pack), `website` (generators for the reference pages of the documentation site and its text checker). |

### Data flow

```
CLI (clap) ─┬─ standalone binary: no JS (TS configs → `node` subprocess)
napi run() ─┘   addon: configs, plugins and reporters run in the host Node
      │
ConfigLoader ──▶ OxcruiseConfig ──▶ Engine::new (compiles + validates every rule, all errors at once)
      │
Walk (ignore::WalkBuilder::build_parallel; exclude applied while walking)
      │
Runtime: one graph thread owns the visited set; rayon workers do
         read → parse (per-thread arena, reset per file) → extract → resolve every dependency
         → mpsc back → the graph thread queues new targets
      │  the arena is dropped right after extraction: facts are kept, ASTs are not
Freeze: sort paths → ModuleId; CSR forward/reverse; leaf modules for package/builtin/unresolved
      │
Analyses, only those some rule asked for: SCC (all edges / runtime-only), distinct dependents
      │
Rules: par_iter over modules and edges, per-task violation buffers (no locks)
       + plugin rules: ONE batched call with the serialised graph
       → inline suppressions → one violation per distinct cycle → sort (severity, location)
      │
CruiseResult ──▶ reporter ──▶ exit code (errors / --deny-warnings / --max-warnings)
```

Determinism: the frozen graph and the output are identical for any thread count (a test compares 1 vs 2 vs 8
threads). `maxDepth` tracks the *minimum* depth of every module, so a module first reached by a long path is
still explored when a shorter one turns up.

### Extraction

The user-facing list of languages and dependency kinds is [Languages](website/src/pages/features/languages.mdx).

| Kind (depcruise name) | Source |
|---|---|
| `import`, `export` | `ModuleRecord.requested_modules` |
| `type-only` | `RequestedModule.is_type`, **or** every specifier of the statement is `type` (`import { type A, type B }`) |
| `dynamic-import` | `import("x")` with a string or an expression-free template literal |
| `require` | `require("x")` |
| `import-equals`, `type-import` | `import x = require()`, `import("x").T` |
| `triple-slash-*` | `/// <reference path|types>` and `<amd-dependency>` before the first statement (`lib` is skipped) |
| `jsdoc*` | `@import {X} from "x"` and `import("x")` in JSDoc |
| `amd-define` | `define([...])` and `require([...])` (what depcruise calls both) |
| `url`, `worker` (new) | `new URL("./x", import.meta.url)`, `new Worker(new URL(...))`; directories (`"."`) are not dependencies |
| `call` (new) | `calls: ["jest.mock", ...]` at the top level of the config (dependency-cruiser's `exoticRequireStrings` too) |

Duplicates are merged on `(specifier, module system, type-only)`; their kinds are unioned, the first occurrence
wins the location. A rule stays suppressed on a merged dependency only if *every* occurrence suppresses it.

`.vue`, `.svelte` and `.astro` files are split into their `<script>` blocks (and Astro frontmatter); every block
is parsed separately and its offsets are mapped back to the file.

#### Symbols

With `ExtractOptions.symbols` the same single pass also records what a file exports and how it uses its imports. It
is **off unless something needs it**: `Engine::wants_symbols()` (`unused-exports` is enabled), a loaded plugin that
declares `needs: ["symbols"]`, `--symbols`, the `symbols` config key, or `cruise({ symbols: true })`. `--format json`
lists exports and names only then. Without it, nothing changes in speed or output.

* Exports come from oxc's `ModuleRecord` (`export { a as b } from` keeps its source name, `import {a}; export {a}` is
  already a re-export there) plus `loc`, per-export suppression comments, the members of exported enums and
  namespaces, whether the file uses the binding itself, and the byte range to delete for `--fix`.
* Each import is classified by the existing AST walk, with no scope analysis: a namespace import read only as `ns.a`
  or an enum only as `E.X` records the dotted names, any other use of the binding is opaque (`All`). Shadowing can
  only make a binding look *more* used, so it never causes a false positive. Single-file components are never narrowed
  (templates use bindings the script does not mention).
* `freeze()` turns that into `Edge.used` (`All`, or a list of names) and `Module.exports`; anything that is not a
  static ES `import`/`export` (`require`, `import()`, JSDoc, AMD, `new URL`) uses `All`. With `manifests` on it also
  reads the nearest `package.json` of every module (`Resolver::nearest_manifest`, the lookup that classifies `npm-*`)
  and finds each dependency's line by scanning the raw JSON.
* **Liveness** (`analyses/exports.rs`) is one forward worklist over `(module, name)` marks: an importing edge marks the
  names it uses, a mark travels through `export { a } from`, `export *` (never `default`) and `export * as ns` to the
  defining module, and each mark is set once, so re-export cycles end. Knip answers the same question by walking up the
  importers once per export; this does the whole graph in one linear pass.
* **Entry points** (`entries.rs`) are the modules whose exports are public: modules no other module imports, the files
  each `package.json` publishes (`main`, `module`, `types`, `bin`, `exports`, mapped from the build output back to
  source with the tsconfig `outDir`/`rootDir`, following relative `extends`; an `extends` that names a package is not
  followed), `index`/`main`/`cli` files, the **conventions** of the tools the
  package declares (`conventions.json`: tests, routes, configs, and the packages those tools load by name; the table is
  in the [rules reference](website/src/pages/reference/rules/index.mdx#conventions)), plugin `entries`, and modules marked `oxcruise-ignore-file`.
* Production mode (`--production`, or `production: true`) leaves tests out as entries and as importers, and skips
  `devDependencies`. `--fix` applies the repairs the rules attach (drop `export`, drop a list entry, drop a
  `package.json` member); edits that overlap wait for the next run.

Compared with [Knip](https://knip.dev) (which also uses oxc-parser and oxc-resolver since v6): the same two phases
(build, then analyse) and the same notion of entries and `OPAQUE` imports, but liveness is linear instead of per
export, and instead of 200+ plugins evaluating tool configs there is one small JSON table of conventions and
`definePlugin({ entries })` for the rest. Not built: class members, catalogs, binaries, JSDoc visibility tags.

### Resolution and classification

* Two `oxc_resolver` instances share config: ESM (`import`, `node`, `default`) and CJS (`require`, ...). A
  dependency written with `require()`/`import =` uses the CJS one, everything else the ESM one, like Node. This is
  a deliberate difference from dependency-cruiser, which merges the conditions (see [Differences](website/src/pages/migration/differences.mdx)).
* Extensions are explicit and ordered source-first, declaration files last, so a package with no `main` resolves to
  `index.js`, not `index.d.ts`. They are never inferred from installed tooling (#988).
* `resolve.conditions: ["source"]` puts the `source` condition first so workspace packages resolve to source, not `dist`.
* Classification: `core` (+`deprecated`), `local`, `npm`/`npm-dev`/`npm-peer`/`npm-optional`/`npm-bundled`/`npm-no-pkg`
  from the nearest `package.json` of the *importing file*; `@types/x` is consulted only if `x` itself is undeclared;
  `aliased-*` by mechanism (config alias, `#imports`, workspace package name, tsconfig `paths`, `baseUrl`).
* Workspaces: npm/yarn/bun `workspaces` and `pnpm-workspace.yaml`; each module records its package.

### Configuration

One file, found in the working directory: `oxcruise.config.ts` or `.oxcruiserc.json`. The user guide is in the
website: [Configuration](website/src/pages/guide/configuration.mdx), [Monorepos](website/src/pages/guide/monorepos.mdx),
[Forbid rules](website/src/pages/features/forbid-rules.mdx) and the generated
[config reference](website/src/pages/reference/config.mdx). Design notes:

* **Schema.** `OxcruiseConfig` (serde + schemars, `deny_unknown_fields`) is the source of truth. The schema
  (`just schema-json`, into `npm/oxcruise/configuration_schema.json`), the TypeScript types (`just config-ts`) and the
  config reference page (`just config-docs`) are generated from it. The doc comments of the config types are the text of
  the reference page and follow [website/STYLE.md](website/STYLE.md).
* **Layering.** `extends` entries are layered first (`OxcruiseConfig::layered` in `crates/oxcruise_config/src/merge.rs`):
  non-empty lists replace, `categories`, `rules` and `resolve.alias` merge by key, `forbid`, `overrides`, `calls` and
  `jsPlugins` concatenate.
* **Paths are globs**, compiled to regexes for the linear-time `regex` crate. `{name}` in a `from` glob captures a path
  segment and is substituted wherever `{name}` appears on the `to` side; a leading `!` negates.
* **`forbid`** is the only declarative rule kind. The compiler validates every rule and reports all errors at once. A
  rule whose `from` or `to.path` matches nothing is reported on stderr. Without `kind` a rule matches every kind of
  target; a `reachable` rule looks at `local` and `package` modules. The module kinds are the same in the config, the
  JSON result and the plugin API.
* **Overrides** change a rule's severity and options for importing modules matching `files`. The engine runs one variant of
  the rule per distinct effective setting, limited to the modules it applies to. A whole-graph finding (`unused-packages`)
  belongs to the module it is reported on: the first module of the package that declares the dependency.
* **CLI-only settings** (`--max-warnings`, `--deny-warnings`, `--max-depth`, `--ignore-pattern`, `--production`, `--fix`)
  are in the [CLI reference](website/src/pages/reference/cli.mdx). `--max-depth` leaves deeper modules out of the graph,
  so every output and rule leaves them out. `--fix` does not chase cascades: run again.
* **Loading.** The standalone binary evaluates a TS or JS config by spawning `node`; the addon evaluates it in the host
  Node. dependency-cruiser configs are not read by a check; `--migrate` converts one.

### Rules

`declare_rule!` generates the metadata (name from the struct, category, version, docs from doc comments); a rule
implements `check_module`, `check_edge` and/or `check_graph`, and declares the analyses it needs in `requirements()`.
Violations go into a per-task buffer, so rules never lock under `par_iter`.

The rule documentation is generated into the website (`just rule-docs`): the doc comments of `declare_rule!` are the text
of the [rules reference](website/src/pages/reference/rules/index.mdx), so they follow
[website/STYLE.md](website/STYLE.md). The categories are described in
[Rules and categories](website/src/pages/features/rules-and-categories.mdx).

Cycles are reported **once per distinct cycle**, not once per edge, and a cycle counts as suppressed when any one of
its edges carries the suppression comment.

### JS plugins

Plugins see the **finished graph**, so there is one batched call per run, not one per file. The user documentation is in
[JS plugins](website/src/pages/features/js-plugins.mdx) and
[Write a JS plugin](website/src/pages/features/writing-js-plugins.mdx); the types are in
`apps/oxcruise/src-js/plugins/types.ts`.

* A rule that declares `meta.schema` gets the same checking as a built-in rule.
* A plugin that throws, or reports something malformed, fails the run with exit code 2.
* Suppression comments apply to plugin rules like any other.

How it works: `run()` (napi, `apps/oxcruise/src/run.rs`) takes four JS callbacks and runs the same `CliRunner` on a
blocking thread. The callbacks take one argument and return a promise of a string: `loadPlugin`, `runRules`
(`{ graph: Buffer, rules: string }`), `renderReport` (`{ name, resultJson }`) and `loadJsConfig`. The graph is
written once into a flat `u32` little-endian buffer (layout documented in `graph_buffer.rs`, round-trip tested against
an independent decoder); the JS side wraps sections in `Uint32Array` views and decodes strings lazily. The bit ↔ name
table for `DepKinds` is generated into TypeScript from the Rust source, with a test that fails when it is stale.

The buffer has two more sections for symbols (version 2): the exports of each module (name, flags, line, column, the
module a re-export reads from and the name it reads) and the names each edge uses; the header says whether symbols
were extracted.

The standalone binary has no JS runtime: with `jsPlugins` set it stops with "need the npm package", and TS/JS
configs are evaluated by spawning `node`.

### Reporters and CLI

The reporters are in `crates/oxcruise_report`; the formats are described in
[Output formats](website/src/pages/features/output-formats.mdx) and the flags in the generated
[CLI reference](website/src/pages/reference/cli.mdx).

The `json` result (and what plugin reporters receive) is versioned (`version`) and uses camelCase names, like the config
and the JS API. **Version 2 renamed these from snake_case**, and renamed the module kinds
`source | stub | core | unresolved` to `local | package | builtin | unresolved`. A dependency's `circular` is true when it
lies on an import cycle of the whole graph, type-only imports included, whether or not a rule reports it; its `cycle` and
`rules` are what the rules reported. `--quiet` hides warnings but `summary.warn` still counts them.
`summary.totalModules` counts the entries of `modules`, which leave out unresolved specifiers, and `dependents` leave out
the module itself.

`oxcruise [paths…]`, plus mode flags that replace it: `--init`, `--migrate[=FILE]`, `--rules` and `--print-config` (one at
a time; `--init` and `--migrate` never overwrite, and no mode takes the flags of a check, except `--rules -f json`).
Exit code 1 for errors, 2 for a bad invocation or config.

## Build and run

```sh
cargo run -p oxcruise -- src            # the standalone binary (no JS runtime; TS configs use a `node` subprocess)
just oxcruise-node-debug                # the napi addon and the bundled JS into npm/oxcruise/dist
just oxcruise-node                      # the same, release build
```

The standalone binary cannot run JS plugins. Use the npm package build for those.

## Check your change

```sh
just ready          # fmt-check, clippy, cargo test, JS typecheck + vitest, visualizer check
```

Or the parts:

| Recipe | Runs |
|---|---|
| `just fmt` | `cargo fmt` and `oxfmt` on JS/TS |
| `just lint` | clippy with `-D warnings`, with and without the `napi` feature |
| `just test` | `cargo test --workspace` |
| `just node-test` | `tsc` and vitest for `apps/oxcruise` (build the addon first with `just oxcruise-node-debug`) |
| `just visualizer-check` | typecheck, test and rebuild the viewer; fails if the committed assets are stale |
| `just smoke` | pack the npm packages, install them in a fresh project and run `npx oxcruise` |
| `just conformance` | diff against dependency-cruiser ([tasks/conformance](tasks/conformance/README.md)) |
| `just bench` | criterion benchmarks ([tasks/benchmark](tasks/benchmark/README.md)) |

The git hooks run `cargo fmt`, `oxfmt` and the README sync on commit. They are fast on purpose; clippy and tests are
CI's job, and `just ready` runs them locally.

Format JS and TS with `pnpm exec oxfmt <files>`. Do not run `oxfmt` without file arguments: it reformats json, toml and
markdown across the repository.

## Generated files

Some files come from other files. Edit the source, regenerate, and commit both. CI fails when they differ.

| Generated | Source | Command |
|---|---|---|
| `npm/oxcruise/configuration_schema.json`, `apps/oxcruise/src-js/config.generated.ts`, `plugins/kinds.generated.ts` | `crates/oxcruise_config/src/config.rs`, `crates/oxcruise_core/src/kinds.rs` | `just config-ts` |
| `website/src` reference pages (rules, config, CLI) | doc comments in `declare_rule!`, `config.rs`, `args.rs` | `just website-gen` |
| `npm/oxcruise/README.md`, `LICENSE` | `README.md`, `LICENSE` | `just sync-readme` |
| `crates/oxcruise_visualize/assets` | `apps/visualizer` | `just visualizer-build` |
| graph-buffer test fixtures | `apps/oxcruise/src/js_plugins/graph_buffer.rs` | `just js-fixtures` |
| insta snapshots | the code under test | `INSTA_UPDATE=always cargo test`, then read the diff (or `cargo insta review` if installed) |

## Common changes

### Add a built-in rule

```sh
just new-rule my_rule
```

The recipe prints the steps: add `crates/oxcruise_rules/src/rules/my_rule.rs` with `declare_rule!` and `impl Rule`, export it
from `rules/mod.rs`, register it in `registry.rs`, and add pass and fail cases to `crates/oxcruise_rules/tests/rules.rs`.
The rule name comes from the struct (`MyRule` becomes `my-rule`). Pick a category (`correctness`, `suspicious`, `style`, `restriction` or `nursery`), declare `requirements()` and the hooks the rule needs, and run `just website-gen` to create its
documentation page. The doc comment is the user documentation. Write it to [website/STYLE.md](website/STYLE.md).

### Add or change a config key

Change the types in `crates/oxcruise_config/src/config.rs`, run `just config-ts`, and update the website page if the key
needs prose. Keep keys few: reuse an existing one if it can do the job.

### Change the plugin API

Update the Rust side, the graph buffer (bump `VERSION` in `graph_buffer.rs` and `graph.ts` if the layout changes, then
`just js-fixtures`), `apps/oxcruise/src-js/plugins/types.ts`, the tests, and the plugin pages in `website/src`.

### Work on the visualizer

`just visualizer-dev` rebuilds the viewer on change into `apps/visualizer/dev-site`. Commit the rebuilt
`crates/oxcruise_visualize/assets` with your change.

### Work on the documentation

`just website-dev` starts the site. The text follows [website/STYLE.md](website/STYLE.md); `just website-lint` checks the
parts that a program can check.

## Performance changes

oxcruise exists to be fast, so a change on the hot path needs numbers. Run `cargo bench` or
`node tasks/benchmark/run.mjs` before and after, put the results in the pull request, and check that the output is
identical (it must not depend on the thread count).

## Pull requests

1. Branch from `master`. Keep a change focused; separate commits for separate ideas.
2. Add or update tests, and docs when behaviour changes.
3. Run `just ready`, and the generator recipes above if you touched their sources.
4. Fill in the pull request template: what changed and why, how to try it, and the numbers for a performance change.

CI (`.github/workflows/ci.yml`) runs on Linux, macOS and Windows for the Rust checks, and on Linux for the Node, generated-file,
website and visualizer checks.

## Reporting bugs

Open an issue with the bug report template: the command you ran, the config, the output, and `oxcruise --version`. A small
repository that reproduces the problem helps most. For a new rule or option, use the feature request template.

## License

By contributing you agree that your contribution is licensed under the [MIT license](LICENSE).
