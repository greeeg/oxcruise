# Benchmark results

Measured 2026-11-05 on a 5-core aarch64 Linux container, Node v24.22.2, oxcruise built with `cargo build --release`
(thin LTO), dependency-cruiser 18.7.0. One machine, one day: treat the shape of the numbers, not the digits, as the result.

## End to end (`node tasks/benchmark/run.mjs`)

Same config for both tools (`no-circular`), same entries, JSON output discarded, best of 3 unless noted.
The repositories are synthetic (`generate.mjs`, deterministic): mostly-acyclic like real code, ~6 imports per module,
about 1 import in 200 pointing "up" and closing a cycle.

| target                                                   | oxcruise (time / peak RSS) | dependency-cruiser 18.7    | speedup |
| -------------------------------------------------------- | -------------------------- | -------------------------- | ------- |
| synthetic 1,000 modules                                  | 0.04 s / 12 MB             | 1.52 s / 175 MB            | 43x     |
| synthetic 5,000 modules                                  | 0.10 s / 35 MB             | 31.31 s / 387 MB           | 323x    |
| synthetic 20,000 modules (1 run)                         | 0.40 s / 121 MB            | > 150 s (timeout) / 669 MB | > 377x  |
| dependency-cruiser's own `src/` (real code, 230 modules) | 0.08 s / 12 MB             | 0.74 s / 129 MB            | 10x     |
| synthetic 50,000 modules                                 | 0.82 s / 293 MB            | not run                    |         |
| synthetic 100,000 modules (600k dependencies)            | 2.38 s / 589 MB            | not run                    |         |

- dependency-cruiser's time grows far faster than linearly here (1.5 s → 31 s from 1k to 5k modules). A first,
  aborted attempt at 20,000 modules had been running for over 9 minutes when it was stopped.
- On small real code the gap is small in absolute terms (0.7 s vs 0.08 s), and dominated by Node start-up.
- At 100k modules, discovery + analysis is about 1 s; the rest is rendering the JSON report.

## Micro benchmarks (`cargo bench`, criterion)

| benchmark                                                                                   | time                 |
| ------------------------------------------------------------------------------------------- | -------------------- |
| extract one TS file, 10 imports (6 KB)                                                      | 44 µs (~137 MiB/s)   |
| extract one TS file, 100 imports (60 KB)                                                    | 431 µs (~142 MiB/s)  |
| extract one TS file, 400 imports (250 KB)                                                   | 2.26 ms (~109 MiB/s) |
| freeze a 50k-module / ~300k-edge graph (sort + CSR)                                         | 25 ms                |
| Tarjan SCC, same graph                                                                      | 6.4 ms               |
| BFS reachability from one module                                                            | 2.8 ms               |
| shortest cycle for **every** cyclic edge, same graph (parallel, `oxcruise_graph` primitive) | 14 s (12.8-15.1 s)   |
| `circular-dependencies` rule on the same graph, default `maxCyclesPerComponent` (100)       | 37 ms                |
| `circular-dependencies` rule on the same graph, `maxCyclesPerComponent: 0` (no limit)       | ~14 s (13.6-14.6 s)  |
| export usage (`no-unused-exports` analysis) on a 50k-module graph with re-exports           | 85 ms                |
| file tree of a 50k-module graph                                                             | 2.2 ms               |
| `folder-structure`, `require` and `allow` with a capture                                    | 6.7 ms               |
| `folder-structure`, a glob that matches every file                                          | 4.8 ms               |
| `visibility` with `folders`                                                                 | 1.6 ms               |
| `circular-dependencies` with `folders`                                                      | 12 ms                |

The graph rows (freeze to BFS) are slower than in the 2026-10-26 run (for example Tarjan 3.8 → 6.4 ms), probably
because `Edge` has since gained a `used` field and is larger. That cause was not isolated, and the criterion intervals
are wide (a rerun of Tarjan gave 9.3 ms, then 6.4 ms), so read these as order of magnitude.

The shortest-cycle row and the two `circular-dependencies` rows use a deliberately pathological graph: random edges make
nearly every module part of one giant strongly connected component, so ~300k breadth-first searches run without a limit.
Real dependency graphs have small components. The rule searches at most `maxCyclesPerComponent` dependencies per
component, so its cost on such a graph is bounded (`cargo bench -p oxcruise_rules`); the rule rows include the whole
engine run, not just the searches.

## Not measured

A large real TypeScript monorepo (for example typescript-eslint) and `hyperfine`-style statistics: both need tooling or
third-party checkouts that this repository does not install on its own. `bench()` in `run.mjs` takes any directory.
