# AGENTS.md

oxcruise validates JS/TS module graphs on the oxc toolchain. Layout, data flow, generated files and workflow:
[CONTRIBUTING.md](CONTRIBUTING.md). Run `just ready` before you finish.

## Performance

Speed is the product. Keep the pipeline parallel and allocation-light.

- **No locks on the hot path.** Per-file work (read → parse → extract → resolve) runs on rayon workers; one graph thread
  owns the visited set. Rules write to per-task violation buffers, never to shared state under `par_iter`.
- **Keep facts, not ASTs.** Drop the oxc arena right after extraction.
- **Analyses are opt-in.** A rule declares `requirements()` and `Rule::HOOKS`; compute nothing nobody asked for.
- **Compile once, match many.** Compile globs and rules in `Engine::new` or cache them (`Arc<Regex>`, `OnceLock`). Never
  build or clone a `Regex` per match. Patterns use the linear-time `regex` crate only.
- **Work on ids, not strings.** Use the frozen CSR graph (`ModuleId`, `EdgeId`, `BitSet`) and `rustc-hash` maps.
- **Plugins get one batched call** over the flat `u32` graph buffer, never one call per file.
- **Output is deterministic** at any thread count (tests compare 1, 2 and 8 threads).
- **Measure.** Back a speed claim with `cargo bench` or `node tasks/benchmark/run.mjs` numbers, and check the output is
  identical.

## API consistency

One concept has one name and one shape in the Rust types, the JSON result, the TS types, the CLI and the docs.

- **Rust config is the source of truth** (`crates/oxcruise_config/src/config.rs`). Never edit generated files; run
  `just config-ts`.
- **JSON output is camelCase.** Change its shape only with a `RESULT_VERSION` bump (`crates/oxcruise_report/src/model.rs`).
- **The graph buffer `VERSION` must match** in `apps/oxcruise/src/js_plugins/graph_buffer.rs` and
  `apps/oxcruise/src-js/plugins/graph.ts`. Bump both, then `just js-fixtures`.
- **Rust and TS plugin APIs mirror each other.** A new field goes in the Rust type, the buffer, `plugins/types.ts` and the docs.
- **Globs only.** `pattern.rs` and `glob.ts` must agree; add cases to `tests/fixtures/glob-cases.json`.
- **Rule, config and CLI doc comments are public docs** and follow [website/STYLE.md](website/STYLE.md).
- **Prefer a smaller surface.** Reuse an existing option, category or kind before you add one.
- **Bad input exits 2 with all errors at once.** Never panic on user input.

## Conventions

- Match the surrounding code: comment density, naming, errors (`thiserror` in libraries, `anyhow` in the app).
- Format JS/TS with `pnpm exec oxfmt <files>`. Never run `oxfmt` without file arguments.
- Add tests with the change: rule cases, insta snapshots, vitest for `src-js`.
