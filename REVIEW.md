# Simplify the config, the rule language and the CLI; speed up `no-circular` and `cruise()`

8 commits, 135 files, +3.9k / −5.5k lines. Mostly a net deletion.

## Why

oxcruise had inherited dependency-cruiser's surface: two ways to write every path (glob or regex), `allow`/`require`/
`orphan`/`numberOf…`/`circular`/`via` rules, presets, `info` severity, a `json-depcruise` reporter, `--compat`,
`check`/`migrate`/`fmt` subcommands. That is a lot to learn, document and keep conformant for little gain, and it made
"good defaults out of the box" impossible. This branch moves to an oxlint-like model: **few knobs, sensible defaults,
one command**. Alongside it, two performance problems:

* `no-circular` on a big strongly connected component runs one BFS per cyclic edge (~20 s on a 50k-module random graph).
* `cruise()` in the JS API shelled out to the CLI, got pretty-printed JSON and parsed it again.

## What changes

**Config and rules** (`oxcruise_config`, `oxcruise_rules`)
* Rules have categories (`correctness` = error, `suspicious` = warn, `style`/`restriction`/`nursery` = off). `extends:
  ["oxcruise:recommended"]` and the presets are gone. An entry in `rules` beats its category.
* Globs are the only pattern language, with `{name}` captures and `!` negation (`to: ["src/features/**",
  "!src/features/{feature}/**"]`). Regex is no longer accepted; the new `regex_glob.rs` converts what it can on migration.
* `forbid` is the only declarative rule kind. `to` takes `path`, `kind`, `typeOnly`, `dynamic`, `relation`, `reachable`.
* Rules renamed and merged: `no-deprecated-core`/`-npm` → `no-deprecated`, `not-to-test`/`not-to-spec` →
  `no-test-imports`, `no-duplicate-dep-types` → `no-duplicate-deps`, `optional-deps-used`/`peer-deps-used` →
  `no-optional-deps`/`no-peer-deps`, `no-non-package-json` → `no-undeclared-deps`.
* `no-circular` ignores type-only edges by default (`includeTypeOnly: true` to keep them). JSDoc imports, triple-slash
  and `new URL()` dependencies are always extracted; `exoticRequireStrings` becomes top-level `calls`.
* `info` severity removed.

**CLI** (`apps/oxcruise`)
* One command: `oxcruise [paths…]`, plus exclusive mode flags `--init`, `--migrate[=FILE]`, `--rules`,
  `--print-config`. Dropped: `check`/`migrate`/`fmt` subcommands, `-o`, `--color`, `--no-config`, `--compat`, `--force`.
* `--init` and `--migrate` never overwrite an existing file. Exit code 1 for errors, 2 for bad invocation/config.
* Reporters: `err` → `default`, `err-long` → `long`; `json-depcruise` removed.
* dependency-cruiser configs are no longer read by a normal run, only converted by `--migrate`, which names what it drops.

**Performance**
* `no-circular` searches at most `maxCyclesPerComponent` (default 100, `0` = unlimited) dependencies per component and
  emits one summary violation. 33 ms vs ~20 s on the pathological graph; `cargo bench -p oxcruise_rules`.
* `cruise()` calls a new napi `runCruise` that returns the graph buffer the plugin host already reads, plus
  violations/summary as small JSON. No stdout capture, no `JSON.parse` of the whole report.
* Smaller: call-expression matching without allocating a name string; violations sorted by borrowed keys.

## Technical choices

* **Breaking by design.** No compatibility shims or deprecation aliases; the project is unpublished, so this is the cheapest
  moment. `--migrate` is the only bridge, and it prefers dropping a rule with a warning over silently widening it.
* **Conformance narrowed to what is still comparable.** `tasks/conformance` now compares modules, edges, types and
  module systems (not violations, since the rule languages no longer overlap). Differences are explained in
  `deviations.json`, e.g. always-on JSDoc/URL extraction yields +231/+215 documented extras on depcruise's own sources.
* **Cap chosen up front, in graph order.** The searched dependencies are picked before the parallel search, so output
  doesn't depend on scheduling and stays identical for any thread count.
* **Binary graph over JSON for the JS API.** Reuses the serialisation the plugin host already needed, so there is one
  graph format. `CliRunner::check` is split into `analyze` (config → graph → rules) and rendering so `runCruise` can
  skip the latter. `ModuleHandle`/`CruiseResult` delegate to the plugin `Module`/`Dependency` types.
* **Schema and TS types are generated** (`just schema-json`, `just config-ts`); `config.generated.ts` and
  `configuration_schema.json` shrink accordingly. Review the Rust types, not the generated files.

## Review pointers

* Start with `crates/oxcruise_config/src/config.rs` and `crates/oxcruise_rules/src/{engine,declarative,pattern}.rs`; they define the new model.
* `crates/oxcruise_config/src/depcruise.rs` + `regex_glob.rs`: the migration, with snapshots for warnings.
* `crates/oxcruise_rules/src/rules/no_circular.rs`: the cap.
* `apps/oxcruise/src/{args,runner,run}.rs`: CLI surface and the `analyze`/render split.
* Snapshot churn (`*.snap`) is large but mechanical: renamed rules, removed `info`, removed `json-depcruise`.

## Known gaps

* Only Linux/arm64 built and tested. Nothing published; the release workflow has never run.
* Migration of depcruise's own `.dependency-cruiser.mjs` was not compared on violations (most of its rules use dropped conditions).
* `ts`, `ts-types` and `vue` conformance fixtures are skipped (they need `typescript` installed).
