# Conformance with dependency-cruiser 18.7

`just conformance` (`tasks/conformance/run.ts`) generates a dependency-cruiser config per target and runs

1. dependency-cruiser 18.7.0 on it, and
2. `oxcruise --migrate` (to a `.json` file) on it, then `oxcruise -f json` with the migrated config (so migration is exercised
   too), with oxcruise's output mapped to dependency-cruiser's shape in the script,

and diffs

* the set of modules,
* the set of dependencies (`from → resolved (specifier)`),
* each dependency's classification types (`local`, `npm-dev`, `aliased-*`, `type-only`, …) and syntax types (`import`,
  `require`, `dynamic-import`, …) separately, and its module system.

Violations are **not** compared: oxcruise's rule language is much smaller, and the rules of the generated config that
use dropped conditions (`circular`, `orphan`, `dependencyTypes`, …) are dropped by `migrate` with a warning. Rule
behaviour is covered by oxcruise's own tests. Any difference that is not listed in `tasks/conformance/deviations.json`
(with its reason) fails the run.

Targets: dependency-cruiser's test fixtures `es6`, `cjs`, `amd`, `exotic-require`, `donotfollow`, `maxDepth`, and its own
`src/` and `tools/`+`bin/` as real code. An `exclude` regex that does not convert to a glob (the two real-code targets
have one) is given as hand-written `excludeGlobs` in `targets.json`.

## Current result

All targets are conformant, with the documented deviations below:

```
ok   es6              modules 5/5  edges 3/3
ok   cjs              modules 36/36  edges 34/34
ok   amd              modules 24/25  edges 29/29  (+3 documented)
ok   exotic-require   modules 2/2  edges 1/1  (+1 documented)
ok   donotfollow / max-depth-1
ok   depcruise-src    modules 229/271  edges 516/673  (+231 documented)
ok   depcruise-tools  modules 253/301  edges 592/727  (+215 documented)
```

## Intentional deviations

1. **Dual ESM/CJS packages** resolve per import syntax (see [migrating](migrating-from-depcruise.md)). The dependency
   exists in both tools; only the file inside `node_modules` differs (`acorn.mjs` vs `acorn.js`).
2. **Merged syntax kinds**: `import` + `export … from` the same module give `[export, import]` where depcruise keeps `[import]`.
3. **AMD ids without `./`** are not resolved against a requirejs `baseUrl`/`paths` config, so `sub/local-module-in-drag`
   is unresolved in oxcruise and resolved in depcruise.
4. **`new URL(…, import.meta.url)`** edges are an oxcruise addition and are always extracted (there is no
   `skipUrls` any more). The report lists them, and the modules reachable only through them, as
   `url-dependency-only-in-oxcruise`: in dependency-cruiser's own `.dependency-cruiser.mjs` that is the whole chain of
   `configs/*.cjs` files (21 in `tools/`+`bin/`, 2 in `src/`).
5. **JSDoc imports** are always extracted. dependency-cruiser detects them only with `detectJSDocImports` **and**
   `typescript` installed, which this run does not have, so these show up as `jsdoc-import-only-in-oxcruise` (199
   modules and edges in dependency-cruiser's own `src/`, 164 in `tools/`+`bin/`: `@import` tags and `import("x")`
   types), and an import that is also written as a JSDoc import gets the extra `jsdoc` and `type-only` kinds.
6. **`exoticRequireStrings`** is merged into `calls`, one kind (`call`). Such a callee counts as an ES import unless its
   name starts with `require`, where dependency-cruiser treats it as a CommonJS require (`module-system-differs`).

## Found by this run and fixed

The conformance run is what found these; each has a regression test:

* AMD `require([…])` is `amd-define`, not `amd-require`.
* Cycles were reported once per edge; depcruise (and readers) want once per cycle.
* A dependency in `dependencies` whose `@types/*` is in `devDependencies` was classified `npm` **and** `npm-dev`, which
  falsely trips `no-duplicate-deps`.
* A package with no `main` resolved to `index.d.ts` instead of `index.js`.
* `new URL(".", import.meta.url)` was treated as a dependency on a directory.

## Not covered

* The `ts`, `ts-types` and `vue` fixtures are skipped: dependency-cruiser itself needs `typescript` (and
  `@vue/compiler-sfc`) installed to parse them, and the conformance task does not install them. Add them to
  `tasks/conformance/package.json` to enable these targets. With `typescript` installed the JSDoc deviation above
  would also go away.
* Larger real repositories (for example typescript-eslint, date-fns) were not run, because that requires cloning and
  installing third-party repositories.
* **Migration check on dependency-cruiser's own config** (`.dependency-cruiser.mjs`): comparing the violations of the
  migrated config against dependency-cruiser's own run was not done; most of its rules use conditions oxcruise
  dropped on purpose.
