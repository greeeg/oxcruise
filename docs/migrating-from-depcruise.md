# Migrating from dependency-cruiser

oxcruise reads your existing `.dependency-cruiser.{json,js,cjs,mjs}` as it is, so you can try it before changing anything:

```sh
npx oxcruise src            # finds .dependency-cruiser.* and says so; same rules, same exit code semantics
npx oxcruise migrate        # writes oxcruise.config.ts (or --json for .oxcruiserc.json)
```

The importer is deliberately conservative. Regexes stay regexes, conditions map one-to-one, and anything it cannot
honour is printed as a **named warning**. A rule that uses an unsupported *condition* is skipped entirely, because
silently dropping the condition would make the rule broader than you wrote it. `migrate` copies each warning into the
generated file as a `// TODO:` comment.

## What maps

| dependency-cruiser | oxcruise |
|---|---|
| `forbidden[]` | `forbid[]` |
| `allowed[]` + `allowedSeverity` | `allow[]` (violations are named `not-in-allowed`) |
| `required[]` | `require[]` |
| `from` / `to`: `path`, `pathNot`, `orphan`, `numberOfDependents{Less,More}Than` | same names; regexes become `{ "regex": … }` |
| `to`: `dependencyTypes(Not)`, `moreThanOneDependencyType`, `couldNotResolve`, `dynamic`, `exoticallyRequired`, `circular`, `via`, `viaOnly`, `reachable: true` | same names |
| `to.preCompilationOnly` | `to.typeOnly` (approximate; oxcruise has exact type-only information) |
| `$1` back-references in `to.path` | still supported in regexes; globs use `{name}` captures instead |
| `options.doNotFollow`, `exclude`, `includeOnly`, `maxDepth` (`0` = unlimited) | `follow.doNotFollow`, `exclude`, `includeOnly`, `follow.maxDepth` |
| `options.tsConfig.fileName` | `resolve.tsconfig` (default `"auto"`: the nearest tsconfig to each file) |
| `options.enhancedResolveOptions` `conditionNames` / `extensions` / `mainFields` | `resolve.conditions` / `extensions` / `mainFields` |
| `options.exoticRequireStrings`, `detectJSDocImports` | `extract.exoticRequireStrings`, `extract.jsdoc` |
| `extends: "dependency-cruiser/configs/recommended-strict"` etc. | `extends: ["oxcruise:strict"]` / `oxcruise:recommended` |

## What does not (yet)

Reported as warnings, by name: `to.license(Not)`, `to.moreUnstable`, `to.ancestor`, `to.exoticRequire(Not)`,
`scope: "folder"` rules, `reachable: false`, `options.baseline` / `knownViolations` / `cache`, `webpackConfig`
(use `resolve.alias`), `combinedDependencies`, `moduleSystems`, `tsPreCompilationDeps: false`.

## Things that behave differently

* **Regex flavour.** oxcruise uses Rust's `regex` (linear time, so no "unsafe regex" guard is needed), which has no
  look-ahead/behind. A pattern such as `^(?!src/)` is reported as an invalid pattern naming the rule; rewrite it with
  `pathNot`.
* **Cycles are reported once per distinct cycle**, not once per edge. A cycle is suppressed if any of its imports
  carries `// oxcruise-ignore-next-line no-circular`.
* **Suppression comments exist**: `// oxcruise-ignore-next-line [rule, …] -- reason`, `// oxcruise-ignore-line`,
  `// oxcruise-ignore-file [rule, …]`.
* **Exit code.** Errors exit `1` (so does `--deny-warnings` with warnings, and `--max-warnings N` exceeded); a bad
  config exits `2`. Pass `--compat` to get dependency-cruiser's behaviour, where the exit code is the number of errors.
* **Dual packages** resolve per import syntax, as Node does: `import` takes a package's `import` export condition,
  `require()` its `require` condition. dependency-cruiser lands on one merged choice, so a module path inside
  `node_modules` can differ.
* **Syntax kinds are merged**: a file that both `import`s and `export … from`s one module has both kinds on that
  dependency.
* **No TypeScript install needed.** oxcruise parses TS, TSX, MTS, CTS, JSX, Vue, Svelte and Astro itself.
* **New dependency forms**: `new URL("./x", import.meta.url)` and `new Worker(new URL(…))` are dependencies
  (`url`, `worker`); turn this off with `extract.skipUrls`.
* **`json-depcruise` output** follows the cruise-result schema, so reporters and tooling that consume it keep
  working. oxcruise's own `json` format is versioned and richer.
* **AMD**: ids without a leading `./` are resolved like package specifiers; requirejs `baseUrl`/`paths` config is
  not read.

## Checking your migration

```sh
npx depcruise src -T json > before.json
npx oxcruise src -f json-depcruise > after.json
```

and compare the `summary.violations` (rule, from, to). The repository's `tasks/conformance` does exactly this against
dependency-cruiser 18.7 on its own fixtures and sources; see [conformance.md](conformance.md).
