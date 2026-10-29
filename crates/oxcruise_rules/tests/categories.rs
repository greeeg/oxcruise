use oxcruise_rules::testing::Tester;
use serde_json::json;

/// A cycle, an orphan and a peer dependency: one problem for a rule of each category.
const GRAPH: &[&str] = &[
    "a.ts -> b.ts",
    "b.ts -> a.ts",
    "lonely.ts",
    "c.ts -> react [npm-peer]",
];

fn run(config: serde_json::Value) -> String {
    let t = Tester::config(config);
    let g = t.graph(GRAPH);
    let r = t.run_graph(&g);
    oxcruise_rules::testing::render(&g, &r)
}

#[test]
fn defaults_are_correctness_error_and_suspicious_warn() {
    // an empty `categories` map still means the defaults
    insta::assert_snapshot!("default_categories", run(json!({ "categories": {} })));
}

#[test]
fn categories_set_the_level_of_their_rules() {
    insta::assert_snapshot!(
        "category_levels",
        run(
            json!({ "categories": { "correctness": "warn", "suspicious": "off", "restriction": "error" } })
        )
    );
}

#[test]
fn an_explicit_rule_wins_over_its_category() {
    let out = run(json!({
        "categories": {},
        "rules": { "no-circular": "off", "no-orphans": "error", "no-peer-deps": "warn" }
    }));
    insta::assert_snapshot!("rule_beats_category", out);
}

#[test]
fn rule_options_apply_on_top_of_the_category() {
    // a rule listed with options is configured by them, whatever its category says
    let out = run(json!({
        "categories": { "suspicious": "off" },
        "rules": { "no-orphans": ["warn", { "ignore": ["lonely.ts"] }] }
    }));
    assert!(!out.contains("no-orphans"), "{out}");
}

#[test]
fn the_old_levels_are_gone() {
    for level in ["info", "ignore"] {
        let err = serde_json::from_value::<oxcruise_config::OxcruiseConfig>(
            json!({ "rules": { "no-circular": level } }),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains(level), "{err}");
    }
}
