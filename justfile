set shell := ["bash", "-cu"]

default:
    @just --list

# Everything CI checks: fmt, clippy, Rust tests, JS typecheck and tests
ready: fmt lint test node-test

fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p oxcruise --lib --features napi -- -D warnings

test:
    cargo test --workspace

# Install JS dependencies (pnpm workspace)
install:
    pnpm install

# regenerate npm/oxcruise/configuration_schema.json from the Rust config types
schema-json:
    cargo run -q -p oxcruise_config --example schema > npm/oxcruise/configuration_schema.json

# regenerate the TypeScript config types from the schema, and the DepKinds table
config-ts: schema-json
    pnpm -C apps/oxcruise run config-ts
    cargo run -q -p oxcruise_core --example kinds_ts > apps/oxcruise/src-js/plugins/kinds.generated.ts

# regenerate the graph-buffer fixtures the JS tests decode
js-fixtures:
    UPDATE_FIXTURES=1 cargo test -q -p oxcruise --lib graph_buffer::fixtures

# Build the native addon (debug) and the bundled JS into npm/oxcruise/dist
oxcruise-node-debug:
    pnpm -C apps/oxcruise run build:binding:debug
    pnpm -C apps/oxcruise run build:js

# Release build of the addon and the bundled JS
oxcruise-node:
    pnpm -C apps/oxcruise run build

# Typecheck and run the vitest suites (needs the addon: `just oxcruise-node-debug`)
node-test:
    pnpm -C apps/oxcruise run typecheck
    pnpm -C apps/oxcruise run test

# Pack the npm packages, install them into a fresh project and run npx oxcruise there
smoke: oxcruise-node-debug
    node tasks/smoke/smoke.mjs

# Diff oxcruise against dependency-cruiser 18.7 (see tasks/conformance/README.md)
conformance:
    cargo build -q -p oxcruise
    node --experimental-strip-types tasks/conformance/run.ts

# Performance comparison and criterion benchmarks (see tasks/benchmark/README.md)
bench:
    cargo bench --workspace

# Generate docs/rules.md from the built-in rule documentation
rule-docs:
    cargo run -q -p oxcruise -- --rules > /tmp/oxcruise-rules.json
    node tasks/website/rules-md.mjs /tmp/oxcruise-rules.json > docs/rules.md

# Start a new built-in rule: prints where to put it
new-rule name:
    @echo "1. add crates/oxcruise_rules/src/rules/{{name}}.rs with declare_rule! + impl Rule"
    @echo "2. export it from crates/oxcruise_rules/src/rules/mod.rs"
    @echo "3. add it to register_rules! in crates/oxcruise_rules/src/registry.rs"
    @echo "4. add pass/fail cases to crates/oxcruise_rules/tests/rules.rs"
