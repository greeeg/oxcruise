# Benchmarks

Two kinds:

* **End to end** (`run.mjs`): wall time and peak memory of oxcruise vs dependency-cruiser on synthetic
  repositories (`generate.mjs`, deterministic) and on dependency-cruiser's own `src/`.
* **Micro** (`cargo bench`): criterion benchmarks for single-file extraction
  (`crates/oxcruise_extract/benches`) and for the graph analyses (`crates/oxcruise_graph/benches`).

```sh
cargo build --release -p oxcruise
node tasks/benchmark/run.mjs                      # sizes 1000,5000,20000, best of 3
node tasks/benchmark/run.mjs --sizes 50000 --runs 1
node tasks/benchmark/generate.mjs /tmp/big 100000 --ts   # a TypeScript repo, oxcruise only
cargo bench
```

Both tools get the same rule (`no-circular`; oxcruise through a small `.oxcruiserc.json`), the same entries, and JSON output. dependency-cruiser needs no
extra parsers for the generated `.js` files. Results from the last run are in [results.md](results.md).

Not measured here: a large real TypeScript monorepo such as typescript-eslint. That needs a clone and an
install of a third-party repository, which this repo's tooling does not do on its own; point `bench()` in
`run.mjs` at a checkout to include one.
