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
fn unused_modules_needs_manifests_but_not_symbols() {
    let e = engine(json!({ "unused-modules": "error" }));
    assert!(!e.wants_symbols());
    assert!(e.wants_manifests());
}

#[test]
fn unused_exports_needs_symbols_and_manifests() {
    let e = engine(json!({ "unused-exports": "warn" }));
    assert!(e.wants_symbols());
    assert!(e.wants_manifests());
}

#[test]
fn unused_packages_needs_manifests_only() {
    let e = engine(json!({ "unused-packages": "error" }));
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
        ["unused-exports", "unused-modules", "unused-packages"]
    );
    assert!(!oxcruise_rules::RuleEnum::has_production_option("nope"));
}
