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
| Type-only filtering doesn't apply across a whole cycle | #695, #305, #899 | `ignoreTypeOnly` drops type-only edges *before* the SCC runs; `via`/`viaOnly` accept edge predicates | done |
| No line numbers, no SARIF/GitHub annotations | #125, #820 | oxc spans → `file:line:col`; `github` reporter | done (SARIF: post-MVP) |
| No custom predicates; JS callbacks refused | #741, #1067, #330, #522 | JS/TS plugin rules over the finished graph | done |
| No relationship predicates | #1067, #624, #628, #960, #986 | `relation` conditions, `internal-modules`, `layers` | done |
| Regex-only paths; unsafe-regex guard; silent dead rules | #472, #651, #980, #806 | globs and the linear-time `regex` crate; `reportDeadRules` | done |
| All-or-nothing cache that relies on git | #715, #1071 | per-file content-hash cache | post-MVP |
| No editor/LSP integration | #529, #272 | LSP with a resident incremental graph | post-MVP |
| No unused exports/dependencies | #674, #468, #638, #340 | symbol-level analysis (export/import names are already extracted) | post-MVP |
| Missing dependency forms | #971, #966, #1007 | `new URL()`/workers, SFC splitter for Vue/Svelte/Astro; CSS `@import`: post-MVP | partly done |

## Repository layout

| Path | What lives there |
|---|---|
| `crates/oxcruise_core` | Data model: `Graph` (forward + reverse CSR, deterministic order), `Module`, `Edge`, `DepKinds` bitflags with depcruise's names, `Violation`, `Severity`, `LineIndex`. No I/O. |
| `crates/oxcruise_extract` | One file → `FileFacts` (dependencies, suppression comments, export/import names). oxc parser + `ModuleRecord` + an AST visitor; SFC script-block splitter. |
| `crates/oxcruise_resolve` | One shared `oxc_resolver` per module system (ESM `import` condition, CJS `require`), dependency classification, package.json section lookup, workspace discovery. |
| `crates/oxcruise_graph` | The parallel runtime (discovery), path matcher, and analyses: Tarjan SCC, shortest cycle per edge, reachability, metrics, bitset. |
| `crates/oxcruise_config` | `OxcruiseConfig` (serde + schemars, `deny_unknown_fields`), JSONC, `extends`/presets, node-subprocess loader behind `JsConfigLoader`, the dependency-cruiser importer. |
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
Freeze: sort paths → ModuleId; CSR forward/reverse; stubs for npm/core/unresolved
      │
Analyses, only those some rule asked for: SCC (all edges / runtime-only), distinct dependents
      │
Rules: par_iter over modules and edges, per-task violation buffers (no locks)
       + plugin rules: ONE batched call with the serialised graph
       → inline suppressions → one violation per distinct cycle → sort (severity, location)
      │
CruiseResult ──▶ reporter ──▶ exit code (errors / --deny-warnings / --max-warnings / --compat)
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
| `exotic-require` | callee names from `extract.exoticRequireStrings` |
| `import-equals`, `type-import` | `import x = require()`, `import("x").T` |
| `triple-slash-*` | `/// <reference path|types>` and `<amd-dependency>` before the first statement (`lib` is skipped) |
| `jsdoc*` (option) | `@import {X} from "x"` and `import("x")` in JSDoc |
| `amd-define` | `define([...])` and `require([...])` (what depcruise calls both) |
| `url`, `worker` (new) | `new URL("./x", import.meta.url)`, `new Worker(new URL(...))`; directories (`"."`) are not dependencies |
| `call` (new) | `extract.calls: ["jest.mock", ...]` |

Duplicates are merged on `(specifier, module system, type-only)`; their kinds are unioned, the first occurrence
wins the location. A rule stays suppressed on a merged dependency only if *every* occurrence suppresses it.

`.vue`, `.svelte` and `.astro` files are split into their `<script>` blocks (and Astro frontmatter); every block
is parsed separately and its offsets are mapped back to the file.

## Resolution and classification

* Two `oxc_resolver` instances share config: ESM (`import`, `node`, `default`) and CJS (`require`, ...). A
  dependency written with `require()`/`import =` uses the CJS one, everything else the ESM one, like Node. This is
  a deliberate difference from dependency-cruiser, which merges the conditions (see [conformance](conformance.md)).
* Extensions are explicit and ordered source-first, declaration files last, so a package with no `main` resolves to
  `index.js`, not `index.d.ts`. They are never inferred from installed tooling (#988).
* `resolve.preferSource` puts the `source` condition first so workspace packages resolve to source, not `dist`.
* Classification: `core` (+`deprecated`), `local`, `npm`/`npm-dev`/`npm-peer`/`npm-optional`/`npm-bundled`/`npm-no-pkg`
  from the nearest `package.json` of the *importing file*; `@types/x` is consulted only if `x` itself is undeclared;
  `aliased-*` by mechanism (config alias, `#imports`, workspace package name, tsconfig `paths`, `baseUrl`).
* Workspaces: npm/yarn/bun `workspaces` and `pnpm-workspace.yaml`; each module records its package.

## Configuration

Discovery in the working directory, in order: `oxcruise.config.{ts,mts,js,mjs}`, `.oxcruiserc.json(c)`,
`.dependency-cruiser.{json,js,cjs,mjs}` (imported, with a note to run `oxcruise migrate`). `--config` overrides.

```ts
// oxcruise.config.ts
import { defineConfig } from "oxcruise";

export default defineConfig({
  extends: ["oxcruise:recommended"],
  entries: ["src"],
  exclude: ["**/*.stories.tsx"],
  resolve: { tsconfig: "auto", preferSource: true },
  rules: {
    "no-circular": ["error", { ignoreTypeOnly: true }],
    "no-dev-deps-in-prod": ["error", { prod: "src/**", test: ["**/*.test.ts"] }],
    layers: ["error", { layers: { ui: "src/ui/**", domain: "src/domain/**" }, allow: { ui: ["domain"], domain: [] } }],
    "acme/max-fan-out": ["warn", { max: 20 }], // plugin rule
  },
  forbid: [
    { name: "feature-isolation", severity: "error",
      from: "src/features/{feature}/**",
      to: { path: "src/features/**", pathNot: "src/features/{feature}/**" } },
    { name: "no-reaching-up", to: { relation: { not: ["descendant-of-from-parent"] } } },
  ],
  overrides: [{ files: ["packages/legacy/**"], rules: { "no-circular": "warn" } }],
  jsPlugins: ["./tools/acme-plugin.ts"],
});
```

* **Paths** are globs, or `{ "regex": "..." }` (Rust `regex` syntax: linear time, no look-around).
* **Captures**: `{name}` in a `from` glob is substituted on the `to` side; regex groups are referenced as `$1`.
* **Relations**: `same-folder`, `descendant-of-from`, `descendant-of-from-parent`, `sibling`, `same-package`, `cross-package`.
* **Cycles**: `circular: true` with `via` (some module other than the importer, or some edge) and `viaOnly` (every
  module of the cycle, importer included, and every edge). Both take module predicates (`path`, `pathNot`) and edge
  predicates (`dependencyTypes`, `typeOnly`, `dynamic`, ...).
* **Layering of configs**: `extends` entries first; pattern lists are replaced by the child when non-empty, sections
  merge field by field, `rules` merge by name, `forbid`/`allow`/`require`/`overrides`/`jsPlugins` concatenate.
* **Overrides** can change a rule's severity and options for importing modules matching `files`; the engine runs
  one variant of the rule per distinct effective setting, limited to the modules it applies to.
* The schema is generated (`just schema-json`) into `npm/oxcruise/configuration_schema.json`; the TypeScript types
  from it (`just config-ts`).

## Rules

`declare_rule!` generates the metadata (name from the struct, category, version, docs from doc comments); a rule
implements `check_module`, `check_edge` and/or `check_graph`, and declares the analyses it needs in `requirements()`.
Violations go into a per-task buffer, so rules never lock under `par_iter`.

Built-in: `no-circular`, `no-orphans`, `no-deprecated-core`, `no-deprecated-npm`, `no-non-package-json`,
`no-unresolvable`, `no-duplicate-dep-types`, `not-to-test`, `not-to-spec`, `no-dev-deps-in-prod`,
`optional-deps-used`, `peer-deps-used`, and new: `layers`, `internal-modules`, `no-cross-package-relative`.
See [rules.md](rules.md).

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
  `Module`: `path`, `kind`, `package`, `dependencies`, `dependents`, `dependentCount`, `is(kind)`.
  `Dependency`: `from`, `to`, `specifier`, `kinds`, `is(kind)`, `loc`, `typeOnly`, `circular`, `cycle`, `unresolved`.
* Specifiers: a relative path, or a package (`acme` is tried as `oxcruise-plugin-acme`, then `acme`). In `jsPlugins`
  an entry can be `{ "name": "team", "specifier": "./acme.ts" }` to register the rules as `team/*`.
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

The standalone binary has no JS runtime: with `jsPlugins` set it stops with "need the npm package", and TS/JS
configs are evaluated by spawning `node`.

## Reporters and CLI

`err` (default; source snippets, underlined specifiers, cycles drawn as loops), `err-long` (adds rule docs), `json`
(native, versioned), `json-depcruise` (cruise-result schema, validated), `github` (annotations), `dot`, `mermaid`,
`text`, and plugin reporters.

`oxcruise [check] [paths…]`, `oxcruise fmt result.json -f dot`, `oxcruise migrate`. Flags: `-c`, `--no-config`,
`-f`, `-o`, `--include-only`, `--exclude`, `--do-not-follow`, `--max-depth`, `--max-warnings`, `--deny-warnings`,
`--compat` (exit code = number of errors), `--threads`, `--rules`, `--color`, `--cwd`.

## Where this differs from the original plan

Honest list, so the plan and the code are not confused:

* CLI parsing is **clap**, not bpaf.
* Cycle violations are deduplicated per cycle (the plan reported per edge); every edge of a reported cycle is still
  flagged `circular` in the result.
* The plugin protocol passes one object per callback (`{ graph, rules }`, `{ name, resultJson }`) because napi
  expands tuples into one array argument. `loadPlugin` is an extra callback, and `jsPlugins` accepts aliases.
* Capture templates are cached in a `Mutex<HashMap>` per pattern, not `papaya`.
* There is **no `RuntimeFileSystem` abstraction** yet (it was groundwork for the LSP).
* `to.reachable: false`, folder-scope rules, `license`, `moreUnstable`, `ancestor`, `exoticRequire(Not)` are not
  supported; the importer skips such rules with a named warning instead of silently widening them.
* Not built: baseline, cache, LSP/`--watch`, `init`, `explain`, SARIF/TeamCity/Azure/JUnit/HTML/CSV/… reporters,
  unused-exports, CSS `@import`.
* The release workflow is drafted but has never run, and nothing has been published.
* Only Linux/arm64 was built and tested while developing; the other platform targets are untested.

See [conformance.md](conformance.md) for the behavioural differences from dependency-cruiser 18.7 and
[migrating-from-depcruise.md](migrating-from-depcruise.md) for moving a project over.
