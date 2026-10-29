# Migrating from dependency-cruiser

oxcruise's config is much smaller than dependency-cruiser's, and it does not read `.dependency-cruiser.*` files when it
checks. `oxcruise --migrate` converts one, and tells you what it could not carry over:

```sh
npx oxcruise --migrate                  # reads .dependency-cruiser.{json,js,cjs,mjs}; writes oxcruise.config.ts
npx oxcruise --migrate -c .oxcruiserc.json   # -c names the file to write; a .json name gives JSON
npx oxcruise --migrate=config/.dependency-cruiser.cjs -c config/oxcruise.config.ts
```

`--migrate` never overwrites: if the target exists it stops with "already exists" (delete the file, or pick another with
`-c`). The `migrate` subcommand and its `--json`, `--force` and `-o/--output` flags are gone.

The conversion is deliberately conservative. A path regex becomes a glob only when it has a common shape (below).
A rule that uses a condition oxcruise does not have, or a regex that does not convert, is **dropped entirely** with a
named warning, because dropping just the condition would make the rule broader than you wrote it. Every warning is
also written into the generated TypeScript file as a `// TODO:` comment. `allowed` and `required` sections are
dropped with a warning too.

## What maps

| dependency-cruiser | oxcruise | If dropped, use instead |
|---|---|---|
| `forbidden[]` | `forbid[]`; `comment` is `message` | |
| `from.path` / `pathNot` (regex) | `from`: a glob or list; `pathNot` becomes a `!glob` in the same list | |
| `to.path` / `pathNot` (regex) | `to`: a glob or list, or `to.path` | |
| `$1` back-references | `{c1}` captures (`from: "src/{c1}/**"`, `to: ["src/**", "!src/{c1}/**"]`) | write `{name}` yourself |
| `to.dynamic`, `to.preCompilationOnly` | `to.dynamic`, `to.typeOnly` (exact type-only information) | |
| `to.couldNotResolve: true` | `to.kind: "unresolved"` (or the `no-unresolvable` rule) | |
| `to.reachable: true` | `to.reachable: true` | |
| `severity` | `severity`: `error`, `warn`, `off`. `info` becomes `warn` (with a warning) | |
| `allowed[]`, `allowedSeverity` | dropped | the `layers` rule, or `forbid` rules |
| `required[]` | dropped | a JS plugin rule |
| `from.orphan`, `to.orphan` | dropped | the `no-orphans` rule |
| `numberOfDependents{Less,More}Than` | dropped | a JS plugin rule |
| `to.dependencyTypes(Not)`, `moreThanOneDependencyType` | dropped | `to.kind`, `to.typeOnly`; rules `no-undeclared-deps`, `no-dev-deps-in-prod`, `no-duplicate-deps`, `no-optional-deps`, `no-peer-deps`, `no-deprecated` |
| `to.circular`, `via`, `viaOnly` | dropped | the `no-circular` rule (`ignore`, `includeTypeOnly`); `overrides.files` to scope it |
| `to.exoticallyRequired`, `license`, `moreUnstable`, `ancestor`; `scope: "folder"` | dropped | a JS plugin rule |
| `options.doNotFollow` | `doNotFollow` (node_modules are always leaves) | |
| `options.exclude` | `ignorePatterns` | |
| `options.includeOnly` | dropped | start from the right `entries`, list the rest in `ignorePatterns` |
| `options.maxDepth` | dropped | the `--max-depth` flag (for `dot` and `mermaid`) |
| `options.tsConfig.fileName` | `resolve.tsconfig` | |
| `enhancedResolveOptions.conditionNames` | `resolve.conditions` (`import`, `require`, `node`, `default` are always tried) | |
| `enhancedResolveOptions.extensions`, `mainFields` | dropped | the resolver defaults |
| `options.exoticRequireStrings` | `calls` | |
| `options.detectJSDocImports`, `tsPreCompilationDeps` | always on | |
| `extends: "dependency-cruiser/configs/recommended"` | the default categories | |
| `extends: ".../recommended-strict"` | `categories: { suspicious: "error" }` | |
| `extends: ".../recommended-warn-only"` | `categories: { correctness: "warn" }` | |
| `extends: "./other.json"` | the other file is converted too and merged into the output | |
| `webpackConfig`, `babelConfig` | dropped | `resolve.alias` |
| `knownViolations`, `cache`, `combinedDependencies`, `moduleSystems` | dropped | |

### Regex to glob

| Regex | Glob |
|---|---|
| `^src/` | `src/**` |
| `^src` | `src*` (a regex without `$` matches `src2/`; so does the glob) |
| `[.]spec[.]ts$` | `**/*.spec.ts` |
| `(^|/)x/` | `**/x/**` |
| `^(src|lib)/` | `{src,lib}/**` |
| `^src/([^/]+)/` | `src/{c1}/**` |
| `a|b` at the top level | two globs |

Anything else (`\d`, `[a-z]`, look-around, quantified groups, …) does not convert and drops the rule or entry, naming
the regex.

### The recommended rules

dependency-cruiser's recommended config is the default of oxcruise: its rules are built in and sorted into categories.

| dependency-cruiser | oxcruise | Category |
|---|---|---|
| `no-circular` | `no-circular` (runtime cycles only) | correctness, `error` |
| `not-to-unresolvable` | `no-unresolvable` | correctness |
| `no-non-package-json` | `no-undeclared-deps` | correctness |
| `not-to-dev-dep` | `no-dev-deps-in-prod` | correctness |
| `not-to-test`, `not-to-spec` | `no-test-imports` | correctness |
| `no-orphans` | `no-orphans` (defaults ignore dotfiles, `.d.ts`, `tsconfig*.json`, `*.config.*`) | suspicious, `warn` |
| `no-deprecated-core`, `no-deprecated-npm` | `no-deprecated` | suspicious |
| `no-duplicate-dep-types` | `no-duplicate-deps` | suspicious |
| `optional-deps-used`, `peer-deps-used` | `no-optional-deps`, `no-peer-deps` | restriction, off |

## Things that behave differently

* **Globs, not regexes.** Rules are matched with the linear-time `regex` crate underneath, so no "unsafe regex" guard
  is needed; globs have `*`, `**`, `?`, `[ab]`, `{a,b}`, and `{name}` captures.
* **Cycles are reported once per distinct cycle**, not once per edge, and only cycles that exist at runtime by default
  (`import type` is erased). A cycle is suppressed if any of its imports carries
  `// oxcruise-ignore-next-line no-circular`.
* **Suppression comments exist**: `// oxcruise-ignore-next-line [rule, …] -- reason`, `// oxcruise-ignore-line`,
  `// oxcruise-ignore-file [rule, …]`.
* **Exit code.** Errors exit `1` (so does `--deny-warnings` with warnings, and `--max-warnings N` exceeded); a bad
  config exits `2`. There is no `--compat`.
* **Output formats** are `default`, `long`, `json`, `github`, `dot`, `mermaid`, `text` and plugin reporters. The
  dependency-cruiser `json` format is gone; consume oxcruise's own versioned `json`.
* **Dual packages** resolve per import syntax, as Node does: `import` takes a package's `import` export condition,
  `require()` its `require` condition. dependency-cruiser lands on one merged choice, so a module path inside
  `node_modules` can differ.
* **Syntax kinds are merged**: a file that both `import`s and `export … from`s one module has both kinds on that
  dependency.
* **No TypeScript install needed.** oxcruise parses TS, TSX, MTS, CTS, JSX, Vue, Svelte and Astro itself.
* **Always extracted**: JSDoc imports (`@import`, `import("x")`), triple-slash directives, and
  `new URL("./x", import.meta.url)` / `new Worker(new URL(…))` (kinds `url`, `worker`).
* **`calls`** replaces `exoticRequireStrings`: the callee's first string argument is a dependency. It resolves as an
  ES import unless the callee's name starts with `require`.
* **AMD**: ids without a leading `./` are resolved like package specifiers; requirejs `baseUrl`/`paths` config is
  not read.

## Checking your migration

```sh
npx oxcruise --migrate -c .oxcruiserc.json
npx depcruise src -T json > before.json
npx oxcruise src -f json > after.json
```

and compare the modules and dependencies. The repository's `tasks/conformance` does exactly this against
dependency-cruiser 18.7 on its own fixtures and sources; see [conformance.md](conformance.md).
