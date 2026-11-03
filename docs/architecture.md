# oxcruise architecture

oxcruise validates and visualises JS/TS module graphs, like [dependency-cruiser](https://github.com/sverweij/dependency-cruiser),
on the oxc toolchain: one parser, parallel everywhere, a modern config, and JS/TS plugins.

## Why: the gaps it closes

Each row is a problem in dependency-cruiser (with the issues that report it) and what oxcruise does about it.
"Status" says what is implemented today.

| Gap in dependency-cruiser | Issues | oxcruise answer | Status |
|---|---|---|---|
| Single-threaded; I/O, parse and resolve are ~76% of runtime | #590, #332, #989, #988 | rayon read → parse → extract → resolve per file; graph analyses on a frozen CSR graph | done |
| Four parsers plus transpile-first; lost `type-only` flags, `.mts`/`.cts` gaps, compiler peer dependencies | #1082, #1104, #588, #873, #1045, #1112, #1054 | one oxc parser; type-only computed from every specifier of a statement; zero peer dependencies | done |
| Only the root tsconfig `paths` are used in monorepos | #565, #981, #575, #947 | nearest tsconfig per importing file (`resolve_file` + `TsconfigDiscovery::Auto`); workspace discovery | done |
| No inline suppression | #188 | `// oxcruise-ignore-next-line <rule>`, `-line`, `-file` | done |
| Type-only filtering doesn't apply across a whole cycle | #695, #305, #899 | `no-circular` drops type-only edges *before* the SCC runs (the default; `includeTypeOnly: true` keeps them) | done |
| No line numbers, no SARIF/GitHub annotations | #125, #820 | oxc spans → `file:line:col`; `github` reporter | done (SARIF: post-MVP) |
| No custom predicates; JS callbacks refused | #741, #1067, #330, #522 | JS/TS plugin rules over the finished graph | done |
| No relationship predicates | #1067, #624, #628, #960, #986 | `relation` conditions, `internal-modules`, `layers` | done |
| Regex-only paths; unsafe-regex guard; silent dead rules | #472, #651, #980, #806 | globs only, matched by the linear-time `regex` crate; rules that match nothing are reported | done |
| All-or-nothing cache that relies on git | #715, #1071 | per-file content-hash cache | post-MVP |
| No editor/LSP integration | #529, #272 | LSP with a resident incremental graph | post-MVP |
| No unused exports/dependencies | #674, #468, #638, #340 | `no-unused-exports`, `no-unused-deps`, `no-unused-files` (see [Symbols](#symbols)) | done (nursery) |
| Missing dependency forms | #971, #966, #1007 | `new URL()`/workers, SFC splitter for Vue/Svelte/Astro; CSS `@import`: post-MVP | partly done |

## Repository layout

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
| `apps/oxcruise` | One crate that is both a binary and a `cdylib`. `CliRunner`, `CliRunResult`, and with the `napi` feature the addon entry points and the plugin host. `src-js/` is the Node side. |
| `npm/oxcruise` | The published package; `npm/<triple>` are the `@oxcruise/binding-*` platform packages. |
| `tasks/` | `conformance` (diff against dependency-cruiser), `benchmark`, `smoke` (npm pack), `website` (generators for the reference pages of the documentation site and its text checker). |

## Data flow

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

## Extraction

The user-facing list of languages and dependency kinds is [Languages](../website/src/pages/features/languages.mdx).

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

### Symbols

With `ExtractOptions.symbols` the same single pass also records what a file exports and how it uses its imports. It
is **off unless something needs it**: `Engine::wants_symbols()` (`no-unused-exports` is enabled), a loaded plugin that
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
  in the [rules reference](../website/src/pages/reference/rules/index.mdx#conventions)), plugin `entries`, and modules marked `oxcruise-ignore-file`.
* Production mode (`--production`, or `production: true`) leaves tests out as entries and as importers, and skips
  `devDependencies`. `--fix` applies the repairs the rules attach (drop `export`, drop a list entry, drop a
  `package.json` member); edits that overlap wait for the next run.

Compared with [Knip](https://knip.dev) (which also uses oxc-parser and oxc-resolver since v6): the same two phases
(build, then analyse) and the same notion of entries and `OPAQUE` imports, but liveness is linear instead of per
export, and instead of 200+ plugins evaluating tool configs there is one small JSON table of conventions and
`definePlugin({ entries })` for the rest. Not built: class members, catalogs, binaries, JSDoc visibility tags.

## Resolution and classification

* Two `oxc_resolver` instances share config: ESM (`import`, `node`, `default`) and CJS (`require`, ...). A
  dependency written with `require()`/`import =` uses the CJS one, everything else the ESM one, like Node. This is
  a deliberate difference from dependency-cruiser, which merges the conditions (see [Differences](../website/src/pages/migration/differences.mdx)).
* Extensions are explicit and ordered source-first, declaration files last, so a package with no `main` resolves to
  `index.js`, not `index.d.ts`. They are never inferred from installed tooling (#988).
* `resolve.conditions: ["source"]` puts the `source` condition first so workspace packages resolve to source, not `dist`.
* Classification: `core` (+`deprecated`), `local`, `npm`/`npm-dev`/`npm-peer`/`npm-optional`/`npm-bundled`/`npm-no-pkg`
  from the nearest `package.json` of the *importing file*; `@types/x` is consulted only if `x` itself is undeclared;
  `aliased-*` by mechanism (config alias, `#imports`, workspace package name, tsconfig `paths`, `baseUrl`).
* Workspaces: npm/yarn/bun `workspaces` and `pnpm-workspace.yaml`; each module records its package.

## Configuration

One file, found in the working directory: `oxcruise.config.ts` or `.oxcruiserc.json`. The user guide is in the
website: [Configuration](../website/src/pages/guide/configuration.mdx), [Monorepos](../website/src/pages/guide/monorepos.mdx),
[Forbid rules](../website/src/pages/features/forbid-rules.mdx) and the generated
[config reference](../website/src/pages/reference/config.mdx). Design notes:

* **Schema.** `OxcruiseConfig` (serde + schemars, `deny_unknown_fields`) is the source of truth. The schema
  (`just schema-json`, into `npm/oxcruise/configuration_schema.json`), the TypeScript types (`just config-ts`) and the
  config reference page (`just config-docs`) are generated from it. The doc comments of the config types are the text of
  the reference page and follow [website/STYLE.md](../website/STYLE.md).
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
  the rule per distinct effective setting, limited to the modules it applies to. A whole-graph finding (`no-unused-deps`)
  belongs to the module it is reported on: the first module of the package that declares the dependency.
* **CLI-only settings** (`--max-warnings`, `--deny-warnings`, `--max-depth`, `--ignore-pattern`, `--production`, `--fix`)
  are in the [CLI reference](../website/src/pages/reference/cli.mdx). `--max-depth` leaves deeper modules out of the graph,
  so every output and rule leaves them out. `--fix` does not chase cascades: run again.
* **Loading.** The standalone binary evaluates a TS or JS config by spawning `node`; the addon evaluates it in the host
  Node. dependency-cruiser configs are not read by a check; `--migrate` converts one.

## Rules

`declare_rule!` generates the metadata (name from the struct, category, version, docs from doc comments); a rule
implements `check_module`, `check_edge` and/or `check_graph`, and declares the analyses it needs in `requirements()`.
Violations go into a per-task buffer, so rules never lock under `par_iter`.

The rule documentation is generated into the website (`just rule-docs`): the doc comments of `declare_rule!` are the text
of the [rules reference](../website/src/pages/reference/rules/index.mdx), so they follow
[website/STYLE.md](../website/STYLE.md). The categories are described in
[Rules and categories](../website/src/pages/features/rules-and-categories.mdx).

Cycles are reported **once per distinct cycle**, not once per edge, and a cycle counts as suppressed when any one of
its edges carries the suppression comment.

## JS plugins

Plugins see the **finished graph**, so there is one batched call per run, not one per file. The user documentation is in
[JS plugins](../website/src/pages/features/js-plugins.mdx) and
[Write a JS plugin](../website/src/pages/features/writing-js-plugins.mdx); the types are in
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

## Reporters and CLI

The reporters are in `crates/oxcruise_report`; the formats are described in
[Output formats](../website/src/pages/features/output-formats.mdx) and the flags in the generated
[CLI reference](../website/src/pages/reference/cli.mdx).

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

## Where this differs from the original plan

Honest list, so the plan and the code are not confused:

* CLI parsing is **clap**, not bpaf.
* Cycle violations are deduplicated per cycle (the plan reported per edge); every edge of a reported cycle is still
  flagged `circular` in the result.
* The plugin protocol passes one object per callback (`{ graph, rules }`, `{ name, resultJson }`) because napi
  expands tuples into one array argument. `loadPlugin` is an extra callback, and `jsPlugins` accepts aliases.
* Capture templates are cached in a `Mutex<HashMap>` per pattern, not `papaya`.
* There is **no `RuntimeFileSystem` abstraction** yet (it was groundwork for the LSP).
* The rule language is deliberately smaller than dependency-cruiser's (`forbid` only, globs only; no `allow`,
  `require`, `orphan`, `numberOf…`, `dependencyTypes`, `circular`/`via`, `license`, folder scope, `reachable: false`).
  `oxcruise --migrate` drops rules that use such conditions with a named warning instead of silently widening them.
* Not built: baseline, cache, LSP/`--watch`, `explain`, SARIF/TeamCity/Azure/JUnit/HTML/CSV/… reporters,
  CSS `@import`. Unused exports, dependencies and files are built (nursery), without Knip's class members, catalogs,
  binaries and JSDoc tags.
* The release workflow is drafted but has never run, and nothing has been published.
* Only Linux/arm64 was built and tested while developing; the other platform targets are untested.

See [Differences](../website/src/pages/migration/differences.mdx) for the behavioural differences from dependency-cruiser
18.7 and [Migrate from dependency-cruiser](../website/src/pages/migration/dependency-cruiser.mdx) for moving a project over.
