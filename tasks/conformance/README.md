# Conformance

Diffs oxcruise against dependency-cruiser 18.7.0. See [docs/conformance.md](../../docs/conformance.md) for what is
compared, the current result and the documented deviations.

```sh
cd tasks/conformance
npm install                                   # dependency-cruiser 18.7.0 and ajv
git clone --depth 1 --branch v18.7.0 https://github.com/sverweij/dependency-cruiser.git .cache/dependency-cruiser
cd ../..
cargo build -p oxcruise
node --experimental-strip-types tasks/conformance/run.ts [--only es6] [--verbose] [--bin target/release/oxcruise]
```

* `targets.json`: what to run (`dir` is relative to the dependency-cruiser checkout). A target with `needs` is skipped
  unless those packages are installed in `tasks/conformance/node_modules`: dependency-cruiser only parses TypeScript and
  Vue when `typescript` / `@vue/compiler-sfc` are present.
* `deviations.json`: intentional differences. `kind` is the diff kind, `match` a regex on `key` (and, for
  `syntax-differs`/`classification-differs`, the detail), `reason` is required reading.
* `.cache/` holds the checkout, the generated per-target configs and `report.json`; it is git-ignored.
