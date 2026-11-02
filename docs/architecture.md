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
| `tasks/` | `conformance` (diff against dependency-cruiser), `benchmark`, `smoke` (npm pack), `website` (rule docs). |

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
  in [rules.md](rules.md#conventions)), plugin `entries`, and modules marked `oxcruise-ignore-file`.
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
  a deliberate difference from dependency-cruiser, which merges the conditions (see [conformance](conformance.md)).
* Extensions are explicit and ordered source-first, declaration files last, so a package with no `main` resolves to
  `index.js`, not `index.d.ts`. They are never inferred from installed tooling (#988).
* `resolve.conditions: ["source"]` puts the `source` condition first so workspace packages resolve to source, not `dist`.
* Classification: `core` (+`deprecated`), `local`, `npm`/`npm-dev`/`npm-peer`/`npm-optional`/`npm-bundled`/`npm-no-pkg`
  from the nearest `package.json` of the *importing file*; `@types/x` is consulted only if `x` itself is undeclared;
  `aliased-*` by mechanism (config alias, `#imports`, workspace package name, tsconfig `paths`, `baseUrl`).
* Workspaces: npm/yarn/bun `workspaces` and `pnpm-workspace.yaml`; each module records its package.

## Configuration

One file, found in the working directory: `oxcruise.config.ts` or `.oxcruiserc.json` (comments and trailing commas
allowed). `--config` names another. dependency-cruiser configs are not read by a check; `oxcruise --migrate` converts
one, and `oxcruise --init` writes a starter config. With neither file, the defaults apply.

```ts
// oxcruise.config.ts
import { defineConfig } from "oxcruise";

export default defineConfig({
  extends: ["../../oxcruise.base.json"],      // file paths only
  entries: ["src"],                            // default ["."]
  ignorePatterns: ["src/generated/**"],        // .gitignore is always honoured; node_modules are always leaves
  doNotFollow: ["**/*.spec.ts"],               // kept in the graph, imports not parsed
  categories: { correctness: "error", suspicious: "warn" },    // this is the default
  rules: {
    "no-circular": "error",
    "no-dev-deps-in-prod": ["error", { prod: ["src/**"], test: ["**/*.test.ts"] }],
    layers: ["error", { layers: { ui: "src/ui/**", domain: "src/domain/**" }, allow: { ui: ["domain"], domain: [] } }],
    "acme/max-fan-out": ["warn", { max: 20 }], // plugin rule
  },
  forbid: [
    { name: "feature-isolation", message: "features must not import each other",
      from: "src/features/{feature}/**",
      to: ["src/features/**", "!src/features/{feature}/**"] },
    { name: "no-node-in-browser", from: "src/web/**", to: { kind: "builtin" } },
    { name: "ui-never-reaches-db", from: "src/ui/**", to: { path: "src/db/**", reachable: true } },
  ],
  overrides: [{ files: ["scripts/**"], rules: { "no-dev-deps-in-prod": "off" } }],
  resolve: { tsconfig: "tsconfig.app.json", conditions: ["source"], alias: { "~": "./src" } },
  calls: ["jest.mock"],                        // callees whose first string argument is a dependency
  jsPlugins: ["./tools/acme-plugin.ts"],
});
```

* **Categories** switch built-in rules on and off together, as in oxlint: `correctness` (default `error`),
  `suspicious` (`warn`), and `style`, `restriction`, `nursery` (off). An entry in `rules` wins over its category; a
  rule that is off by its category can still be enabled there, with options.
* **Levels** are `off`, `warn`, `error`. There is no `info`.
* **Paths are globs**, in one string or a list. `{name}` in a `from` glob captures a path segment and is substituted
  wherever `{name}` appears on the `to` side. A leading `!` negates: a list matches when some positive glob matches and
  no negated one does, and a list of only negations means "everything except". Negated globs may use captures.
* **`forbid`** is the only declarative rule kind: `from` (globs) and `to` (globs, or an object with `path`, `kind`
  (`local`, `package`, `builtin`, `unresolved`), `typeOnly`, `dynamic`, `relation`, `reachable`). The level is the
  rule's own `severity` (default `error`); `overrides` may change it per file by name. A rule whose `from` or
  `to.path` matches nothing is reported on stderr. Without `kind` a rule matches every kind of target, built-in
  modules and unresolved specifiers included, except a `reachable` rule, which looks at `local` and `package`
  modules. The module kinds are the same in the config, the JSON result and the plugin API.
* **Relations**: `same-folder`, `descendant-of-from`, `descendant-of-from-parent`, `sibling`, `same-package`,
  `cross-package`; a list means any of them.
* **Resolution**: `resolve.conditions` are tried before `import`/`require`, `node`, `default`; `resolve.tsconfig`
  names one tsconfig for every file (default: the nearest one per file).
* **Always on**: JSDoc imports, triple-slash directives, `new URL(..., import.meta.url)` dependencies.
* `entries`, `ignorePatterns`, `doNotFollow` and `overrides[].files` take one glob or a list, like `forbid.from`.
* **Layering of configs**: `extends` entries first; lists are replaced by the child when non-empty, `categories`,
  `rules` and `resolve.alias` merge by key (the child wins), `forbid`/`overrides`/`calls`/`jsPlugins` concatenate.
* **Overrides** can change a rule's severity and options for importing modules matching `files`; the engine runs
  one variant of the rule per distinct effective setting, limited to the modules it applies to.
* **CLI-only**: `--max-warnings`, `--deny-warnings`, `--max-depth` (deeper modules are not in the graph, so every output and rule leaves them out), `--ignore-pattern`,
  `--production` and `--fix` (for the `no-unused-*` rules; `--fix` is refused with `--init`, `--migrate`, `--rules`
  and `--print-config`, and does not chase cascades: run again).
* The schema is generated (`just schema-json`) into `npm/oxcruise/configuration_schema.json`; the TypeScript types
  from it (`just config-ts`).

## Rules

`declare_rule!` generates the metadata (name from the struct, category, version, docs from doc comments); a rule
implements `check_module`, `check_edge` and/or `check_graph`, and declares the analyses it needs in `requirements()`.
Violations go into a per-task buffer, so rules never lock under `par_iter`.

Built-in, by category: **correctness** `no-circular` (runtime cycles; `includeTypeOnly`, `ignore`),
`no-unresolvable`, `no-undeclared-deps`, `no-dev-deps-in-prod`, `no-test-imports`, `no-cross-package-relative`;
**suspicious** `no-orphans`, `no-deprecated`, `no-duplicate-deps`; **restriction** `no-optional-deps`, `no-peer-deps`,
`layers`, `internal-modules`; **nursery** `no-unused-exports`, `no-unused-deps`, `no-unused-files`
(see [Symbols](#symbols)). See [rules.md](rules.md).

Cycles are reported **once per distinct cycle**, not once per edge, and a cycle counts as suppressed when any one of
its edges carries the suppression comment.

## JS plugins

Plugins see the **finished graph**, so there is one batched call per run, not one per file.

```ts
// tools/acme-plugin.ts
import { definePlugin } from "oxcruise/plugin";

export default definePlugin({
  meta: { name: "acme" },
  rules: {
    "max-fan-out": {
      meta: { docs: "Limit local fan-out", schema: { max: { type: "number", default: 15 } } },
      create(ctx) {
        return {
          module(m) {
            const n = m.dependencies.filter((d) => d.is("local")).length;
            if (n > ctx.options.max) ctx.report({ module: m, message: `fan-out ${n} > ${ctx.options.max}` });
          },
          // dependency(d) {}, graph(g) {}, before(g) {}, after(g) {}
        };
      },
    },
  },
  reporters: { "json-lines": (result) => result.violations.map((v) => JSON.stringify(v)).join("\n") + "\n" },
});
```

* `Graph`: `modules`, `edges`, `module(path)`, `cycles()`, `reach(from)`, `packages`, `folders`.
  `Module`: `path`, `kind`, `package`, `dependencies`, `dependents`, `dependentCount`, `exports`, `is(kind)`.
  `Dependency`: `from`, `to`, `specifier`, `kinds`, `is(kind)`, `loc`, `typeOnly`, `circular`, `cycle`, `unresolved`,
  `names`. Symbols (see [Symbols](#symbols)) are present when the plugin declares `needs: ["symbols"]` (check
  `graph.symbols`): `Module.exports` is
  `{ module, name, kind, isType, used, loc, from, imported }[]`, `Dependency.names` is `"*"` or the names used, and
  `graph.importersOf(module, name)` lists the modules that import an export, following re-exports. `used` does not
  depend on any rule's options: it is computed over the shared entry points.
* `ctx.report({ module | dependency | export, message })`: a rule can report on a module, a dependency, or an export
  (`needs: ["symbols"]`); `ctx.severity` is the level the rule is configured with.
* A plugin can declare entry points with static globs, `definePlugin({ meta, entries: ["**/*.stories.tsx"], … })`;
  they are public API for `Export.used`, `no-unused-exports` and `no-unused-files`, like conventions.
* Specifiers: a relative path, or a package (`acme` is tried as `oxcruise-plugin-acme`, then `acme`). In `jsPlugins`
  an entry can be `{ "name": "team", "specifier": "./acme.ts" }` to register the rules as `team/*`.
* A rule that declares `meta.schema` gets the same checking as a built-in rule: an option that is not in the schema,
  or whose `typeof` is not its `type` (`array` for lists), stops the run with the rule and option named.
  `Module.dependencies` are sorted by target path, then specifier.
* Rules are configured as `plugin/rule`; reporters are selected with `-f plugin/reporter`.
* TypeScript plugins and configs run through Node's native type stripping (Node ≥ 22.18).
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

`default` (source snippets, underlined specifiers, cycles drawn as loops), `long` (adds rule docs), `json`
(native, versioned), `github` (annotations), `dot`, `mermaid`, `text`, and plugin reporters (`-f plugin/reporter`).

The `json` result (and what plugin reporters receive) uses camelCase names, like the config and the JS API
(`notFollowed`, `toKind`, `moduleSystem`, `isType`, `specifierLen`, `parseError`, `summary.totalModules`).
**Version 2 renamed these from snake_case**, and renamed the module kinds `source | stub | core | unresolved`
(`kind`, `toKind`) to `local | package | builtin | unresolved`, the names `forbid[].to.kind` and the JS API use; check
`version` before reading older saved results. A dependency's `circular` is true when it lies on an import cycle of the
whole graph, type-only imports included, whether or not a rule reports it; its `cycle` and `rules` are what the rules
reported. `--quiet` hides warnings but `summary.warn` still counts them. An export has `line`/`col` only when its
location is known, and `from`/`imported` when it re-exports. A violation's `kind`
is `dependency`, `module`, `cycle`, `reachability`, `export` (`from` is the module) or `file` (`from` is the path of a
`package.json`). `summary.totalModules` counts the entries of `modules`, which leave out unresolved specifiers, and
`dependents` leave out the module itself.

`oxcruise [paths…]`, plus mode flags that replace it: `--init`, `--migrate[=FILE]`, `--rules` and `--print-config`
(one at a time; `--init` and `--migrate` never overwrite, and no mode takes the flags of a check, except `--rules -f json`). Flags: `-c`, `-f`, `--quiet`, `--ignore-pattern`,
`--max-depth`, `--max-warnings`, `--deny-warnings`, `--production`, `--fix`, `--threads`, `--cwd`. Color follows the terminal, `NO_COLOR` and
`FORCE_COLOR`. Exit code 1 for
errors, 2 for a bad invocation or config.

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

See [conformance.md](conformance.md) for the behavioural differences from dependency-cruiser 18.7 and
[migrating-from-depcruise.md](migrating-from-depcruise.md) for moving a project over.
