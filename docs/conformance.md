# Conformance with dependency-cruiser 18.7

`just conformance` (`tasks/conformance/run.ts`) runs dependency-cruiser 18.7.0 and oxcruise on the **same**
dependency-cruiser config (oxcruise reads it through its importer, so migration is exercised too), then diffs

* the set of modules,
* the set of dependencies (`from → resolved (specifier)`),
* each dependency's classification types (`local`, `npm-dev`, `aliased-*`, `type-only`, …) and syntax types (`import`,
  `require`, `dynamic-import`, …) separately,
* the set of violations `(rule, from, to)`,

and validates both outputs against dependency-cruiser's own `cruise-result.schema.json`. Any difference that is not
listed in `tasks/conformance/deviations.json` (with its reason) fails the run.

Targets: dependency-cruiser's test fixtures `es6`, `cjs`, `amd`, `exotic-require`, `donotfollow`, `maxDepth`, and its own
`src/` and `tools/`+`bin/` as real code.

## Current result

All targets are conformant, with the documented deviations below:

```
ok   es6              modules 5/5  edges 3/3  violations 2/2
ok   cjs              modules 36/36  edges 34/34  violations 7/7
ok   amd              modules 24/25  edges 29/29  violations 5/7
ok   exotic-require / donotfollow / max-depth-1
ok   depcruise-src    modules 229/230  edges 516/516  violations 6/6
ok   depcruise-tools  modules 253/254  edges 592/592  violations 6/6
```

## Intentional deviations

1. **Dual ESM/CJS packages** resolve per import syntax (see [migrating](migrating-from-depcruise.md)). The dependency
   exists in both tools; only the file inside `node_modules` differs (`acorn.mjs` vs `acorn.js`).
2. **Merged syntax kinds**: `import` + `export … from` the same module give `[export, import]` where depcruise keeps `[import]`.
3. **AMD ids without `./`** are not resolved against a requirejs `baseUrl`/`paths` config, so `sub/local-module-in-drag`
   is unresolved in oxcruise and resolved in depcruise.
4. **`new URL(…, import.meta.url)`** edges are an oxcruise addition; the conformance wrapper config switches them off
   (`extract.skipUrls`).

## Found by this run and fixed

The conformance run is what found these; each has a regression test:

* `exotic-require` carried `require` too; AMD `require([…])` is `amd-define`, not `amd-require`.
* Cycles were reported once per edge; depcruise (and readers) want once per cycle.
* A dependency in `dependencies` whose `@types/*` is in `devDependencies` was classified `npm` **and** `npm-dev`, which
  falsely trips `no-duplicate-dep-types`.
* A package with no `main` resolved to `index.d.ts` instead of `index.js`.
* `json-depcruise` lacked `circular` on every dependency, did not list unresolved modules, kept `node:` on core modules and
  leaked oxcruise-only kinds, all of which fail the schema or break consumers.
* `new URL(".", import.meta.url)` was treated as a dependency on a directory.

## Not covered

* The `ts`, `ts-types` and `vue` fixtures are skipped: dependency-cruiser itself needs `typescript` (and
  `@vue/compiler-sfc`) installed to parse them, and the conformance task does not install them. Add them to
  `tasks/conformance/package.json` to enable these targets.
* Larger real repositories (for example typescript-eslint, date-fns) were not run, because that requires cloning and
  installing third-party repositories.
* **Migration check on dependency-cruiser's own config** (`.dependency-cruiser.mjs`): `oxcruise migrate` ran and listed
  what it cannot map (`to.ancestor`, `exoticRequireNot`, `licenseNot`, `baseline`, `cache`, …), but comparing the
  violations of the migrated config against dependency-cruiser's own run was not completed.
