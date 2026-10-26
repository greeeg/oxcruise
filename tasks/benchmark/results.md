# Benchmark results

Measured 2026-10-26 on a 5-core aarch64 Linux container, Node v24.22.1, oxcruise built with `cargo build --release`
(thin LTO), dependency-cruiser 18.7.0. One machine, one day: treat the shape of the numbers, not the digits, as the result.

## End to end (`node tasks/benchmark/run.mjs`)

Same config for both tools (`no-circular`), same entries, JSON output discarded, best of 3 unless noted.
The repositories are synthetic (`generate.mjs`, deterministic): mostly-acyclic like real code, ~6 imports per module,
about 1 import in 200 pointing "up" and closing a cycle.

| target | oxcruise (time / peak RSS) | dependency-cruiser 18.7 | speedup |
|---|---|---|---|
| synthetic 1,000 modules | 0.03 s / 13 MB | 1.46 s / 175 MB | 49x |
| synthetic 5,000 modules | 0.08 s / 39 MB | 30.07 s / 384 MB | 362x |
| synthetic 20,000 modules (1 run) | 0.31 s / 137 MB | > 150 s (timeout) / 671 MB | > 478x |
| dependency-cruiser's own `src/` (real code, 230 modules) | 0.06 s / 12 MB | 0.61 s / 130 MB | 9x |
| synthetic 50,000 modules | 0.87 s / 333 MB | not run | |
| synthetic 100,000 modules (600k dependencies) | 2.47 s / 668 MB | not run | |

* dependency-cruiser's time grows far faster than linearly here (1.5 s → 30 s from 1k to 5k modules). A first,
  aborted attempt at 20,000 modules had been running for over 9 minutes when it was stopped.
* On small real code the gap is small in absolute terms (0.6 s vs 0.06 s), and dominated by Node start-up.
* At 100k modules, discovery + analysis is about 1 s; the rest is rendering the JSON report.

## Micro benchmarks (`cargo bench`, criterion)

| benchmark | time |
|---|---|
| extract one TS file, 10 imports (6 KB) | 53 µs (~113 MiB/s) |
| extract one TS file, 100 imports (60 KB) | 530 µs (~115 MiB/s) |
| extract one TS file, 400 imports (250 KB) | 2.25 ms (~110 MiB/s) |
| freeze a 50k-module / ~300k-edge graph (sort + CSR) | 21 ms |
| Tarjan SCC, same graph | 3.8 ms |
| BFS reachability from one module | 2.0 ms |
| shortest cycle for **every** cyclic edge, same graph (parallel) | 8.3 s |

The last row is a deliberately pathological graph: random edges make nearly every module part of one giant strongly
connected component, so ~300k breadth-first searches run. Real dependency graphs have small components; the
benchmark exists to show the worst case is bounded, and a bidirectional search would be the next optimisation.

## Not measured

A large real TypeScript monorepo (for example typescript-eslint) and `hyperfine`-style statistics: both need tooling or
third-party checkouts that this repository does not install on its own. `bench()` in `run.mjs` takes any directory.
