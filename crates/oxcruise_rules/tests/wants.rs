//! What a configuration makes `cruise` collect.

use oxcruise_config::OxcruiseConfig;
use oxcruise_rules::Engine;
use serde_json::{Value, json};

fn engine(rules: Value) -> Engine {
    let config: OxcruiseConfig = serde_json::from_value(json!({
        "categories": { "correctness": "off", "suspicious": "off" },
        "rules": rules,
    }))
    .unwrap();
    Engine::new(&config, None).unwrap()
}

#[test]
fn unused_files_needs_manifests_but_not_symbols() {
    let e = engine(json!({ "no-unused-files": "error" }));
    assert!(!e.wants_symbols());
    assert!(e.wants_manifests());
}

#[test]
fn unused_exports_needs_symbols_and_manifests() {
    let e = engine(json!({ "no-unused-exports": "warn" }));
    assert!(e.wants_symbols());
    assert!(e.wants_manifests());
}

#[test]
fn unused_deps_needs_manifests_only() {
    let e = engine(json!({ "no-unused-deps": "error" }));
    assert!(!e.wants_symbols());
    assert!(e.wants_manifests());
}

#[test]
fn no_rules_need_nothing() {
    let none = engine(json!({}));
    assert!(!none.wants_symbols());
    assert!(!none.wants_manifests());
}

#[test]
fn only_the_unused_rules_have_a_production_option() {
    let with: Vec<_> = oxcruise_rules::builtin_rules()
        .into_iter()
        .filter(|r| oxcruise_rules::RuleEnum::has_production_option(r.name))
        .map(|r| r.name)
        .collect();
    assert_eq!(
        with,
        ["no-unused-deps", "no-unused-exports", "no-unused-files"]
    );
    assert!(!oxcruise_rules::RuleEnum::has_production_option("nope"));
}
