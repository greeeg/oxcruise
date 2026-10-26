set shell := ["bash", "-cu"]

default:
    @just --list

# fmt + clippy + test
ready: fmt lint test

fmt:
    cargo fmt --all

lint:
    cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

# regenerate npm/oxcruise/configuration_schema.json
schema-json:
    cargo run -q -p oxcruise_config --example schema > npm/oxcruise/configuration_schema.json

# regenerate TS types from the schema
config-ts:
    pnpm -C apps/oxcruise run config-ts

new-rule name:
    @echo "add crates/oxcruise_rules/src/rules/{{name}}.rs and register it in rules.rs"

conformance:
    node --experimental-strip-types tasks/conformance/run.ts

bench:
    cargo bench --workspace

oxcruise-node:
    pnpm -C apps/oxcruise run build
