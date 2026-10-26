use oxcruise_rules::testing::Tester;
use serde_json::json;

fn t(config: serde_json::Value) -> Tester {
    Tester::config(config)
}

#[test]
fn forbid_by_path_with_glob_captures() {
    // features may not import each other
    let tester = t(json!({
        "forbid": [{
            "name": "feature-isolation", "severity": "error",
            "from": "src/features/{feature}/**",
            "to": { "path": "src/features/**", "pathNot": "src/features/{feature}/**" }
        }]
    }));
    tester.pass(&[
        "src/features/a/x.ts -> src/features/a/y.ts",
        "src/features/a/x.ts -> src/shared/z.ts",
        "src/other/x.ts -> src/features/a/y.ts",
    ]);
    insta::assert_snapshot!(
        "forbid_feature_isolation",
        tester.fail(&[
            "src/features/a/x.ts -> src/features/b/y.ts",
            "src/features/a/x.ts -> src/features/a/ok.ts"
        ])
    );
}

#[test]
fn imported_depcruise_regex_backreferences() {
    // what the importer produces for `"to": { "path": "^src/$1/" }`
    let tester = t(json!({
        "forbid": [{
            "name": "stay-in-your-lane",
            "from": { "path": { "regex": "^src/([^/]+)/" } },
            "to": { "path": { "regex": "^src/" }, "pathNot": { "regex": "^src/$1/" } }
        }]
    }));
    tester.pass(&["src/a/x.ts -> src/a/y.ts"]);
    tester.fail(&["src/a/x.ts -> src/b/y.ts"]);
}

#[test]
fn relation_conditions() {
    // no reaching up out of your parent folder (#624, #628)
    let tester = t(json!({
        "forbid": [{ "name": "no-reaching-up", "from": { "path": "src/**" }, "to": { "relation": { "not": ["descendant-of-from-parent"] } } }]
    }));
    tester.pass(&["src/a/x.ts -> src/a/y.ts", "src/a/x.ts -> src/b/y.ts"]);
    insta::assert_snapshot!(
        "relation_reaching_up",
        tester.fail(&["src/a/x.ts -> lib/z.ts"])
    );

    let sib =
        t(json!({ "forbid": [{ "name": "no-siblings", "to": { "relation": ["sibling"] } }] }));
    sib.pass(&["src/a/x.ts -> src/a/y.ts"]);
    sib.fail(&["src/a/x.ts -> src/b/y.ts"]);
}

#[test]
fn dependency_type_conditions() {
    let tester = t(json!({
        "forbid": [
            { "name": "no-dev", "to": { "dependencyTypes": ["npm-dev"], "dependencyTypesNot": ["type-only"] } },
            { "name": "no-dynamic", "to": { "dynamic": true } },
            { "name": "no-unresolved", "to": { "couldNotResolve": true } },
        ]
    }));
    tester.pass(&["a.ts -> x [npm-dev type-only]", "a.ts -> b.ts"]);
    insta::assert_snapshot!(
        "dependency_type_conditions",
        tester.fail(&[
            "a.ts -> x [npm-dev]",
            "a.ts -> b.ts [dynamic-import]",
            "a.ts -> ./nope [unknown]"
        ])
    );
    let err = oxcruise_rules::Engine::new(
        &serde_json::from_value(
            json!({ "forbid": [{ "name": "r", "to": { "dependencyTypes": ["npn"] } }] }),
        )
        .unwrap(),
        None,
    )
    .err()
    .unwrap();
    assert!(err[0].contains("unknown dependency type `npn`"), "{err:?}");
}

#[test]
fn module_rules_orphan_and_dependents() {
    let tester = t(json!({
        "forbid": [
            { "name": "orphans", "severity": "warn", "from": { "orphan": true, "pathNot": "**/*.d.ts" }, "to": {} },
            { "name": "popular", "severity": "info", "from": { "numberOfDependentsMoreThan": 2 } },
        ]
    }));
    insta::assert_snapshot!(
        "module_rules",
        tester.fail(&[
            "a.ts -> hub.ts",
            "b.ts -> hub.ts",
            "c.ts -> hub.ts",
            "lonely.ts",
            "types.d.ts"
        ])
    );
}

#[test]
fn circular_with_via_and_via_only() {
    // a cycle that goes through src/shared is forbidden; elsewhere it is fine
    let via = t(
        json!({ "forbid": [{ "name": "via-shared", "to": { "circular": true, "via": { "path": "src/shared/**" } } }] }),
    );
    via.pass(&["src/a.ts -> src/b.ts", "src/b.ts -> src/a.ts"]);
    insta::assert_snapshot!(
        "circular_via",
        via.fail(&[
            "src/a.ts -> src/shared/s.ts",
            "src/shared/s.ts -> src/b.ts",
            "src/b.ts -> src/a.ts"
        ])
    );

    // viaOnly: every step must match
    let only = t(
        json!({ "forbid": [{ "name": "only-in-feature", "to": { "circular": true, "viaOnly": { "path": "src/feature/**" } } }] }),
    );
    only.pass(&[
        "src/feature/a.ts -> src/other/b.ts",
        "src/other/b.ts -> src/feature/a.ts",
    ]);
    only.fail(&[
        "src/feature/a.ts -> src/feature/b.ts",
        "src/feature/b.ts -> src/feature/a.ts",
    ]);

    // edge predicates on the cycle path (#695, #763): only cycles that contain a static edge
    let edge = t(
        json!({ "forbid": [{ "name": "static-cycle", "to": { "circular": true, "via": { "dynamic": false } } }] }),
    );
    edge.fail(&["a.ts -> b.ts", "b.ts -> a.ts"]);
    let lazy = t(
        json!({ "forbid": [{ "name": "cycle-without-dynamic", "to": { "circular": true, "viaOnly": { "dynamic": false } } }] }),
    );
    lazy.pass(&["a.ts -> b.ts [dynamic-import]", "b.ts -> a.ts"]);
    lazy.fail(&["a.ts -> b.ts", "b.ts -> a.ts"]);

    let bare_via = oxcruise_rules::Engine::new(
        &serde_json::from_value(
            json!({ "forbid": [{ "name": "r", "to": { "via": { "path": "x" } } }] }),
        )
        .unwrap(),
        None,
    )
    .err()
    .unwrap();
    assert!(
        bare_via[0].contains("need `circular: true`"),
        "{bare_via:?}"
    );
}

#[test]
fn circular_ignoring_type_only_through_options() {
    let tester = t(json!({
        "options": { "cycles": { "ignoreTypeOnly": true } },
        "forbid": [{ "name": "no-cycles", "to": { "circular": true } }]
    }));
    tester.pass(&["a.ts -> b.ts", "b.ts -> a.ts [type-only]"]);
    tester.fail(&["a.ts -> b.ts", "b.ts -> a.ts"]);
}

#[test]
fn reachable() {
    let tester = t(json!({
        "forbid": [{ "name": "ui-must-not-reach-db", "from": { "path": "src/ui/**" }, "to": { "path": "src/db/**", "reachable": true } }]
    }));
    tester.pass(&[
        "src/ui/a.ts -> src/domain/b.ts",
        "src/domain/b.ts -> src/other/c.ts",
        "src/db/q.ts",
    ]);
    insta::assert_snapshot!(
        "reachable",
        tester.fail(&[
            "src/ui/a.ts -> src/domain/b.ts",
            "src/domain/b.ts -> src/db/q.ts",
            "src/db/q.ts -> src/db/conn.ts"
        ])
    );
}

#[test]
fn allow_list() {
    let tester = t(json!({
        "allow": [
            { "name": "x", "severity": "warn", "from": { "path": "src/**" }, "to": { "path": "src/**" } },
            { "name": "y", "severity": "warn", "from": { "path": "src/**" }, "to": { "dependencyTypes": ["core"] } },
        ]
    }));
    tester.pass(&["src/a.ts -> src/b.ts", "src/a.ts -> fs [core]"]);
    let out = tester.fail(&["src/a.ts -> lib/c.ts", "src/a.ts -> lodash [npm]"]);
    insta::assert_snapshot!("allow_list", out);
}

#[test]
fn require() {
    let tester = t(json!({
        "require": [{ "name": "pages-use-layout", "module": { "path": "src/pages/*.tsx" }, "to": { "path": "src/layout/**" } }]
    }));
    tester.pass(&["src/pages/home.tsx -> src/layout/main.tsx", "src/other.tsx"]);
    insta::assert_snapshot!(
        "require",
        tester.fail(&[
            "src/pages/home.tsx -> src/shared/x.ts",
            "src/pages/about.tsx -> src/layout/main.tsx"
        ])
    );
}

#[test]
fn severity_override_and_off_for_declarative_rules() {
    let rules = json!([{ "name": "r", "severity": "error", "to": { "path": "bad.ts" } }]);
    let out = t(json!({ "forbid": rules, "rules": { "r": "warn" } })).fail(&["a.ts -> bad.ts"]);
    assert!(out.starts_with("warn r"), "{out}");
    t(json!({ "forbid": rules, "rules": { "r": "off" } })).pass(&["a.ts -> bad.ts"]);
}

#[test]
fn suppression_comments() {
    let tester = t(json!({ "rules": { "no-circular": "error", "no-orphans": "error" } }));
    // suppressing one rule on an edge leaves the other edge's violation alone
    let out = tester.fail(&["a.ts -> b.ts [ignore=no-circular]", "b.ts -> a.ts"]);
    insta::assert_snapshot!("suppression_edge", out);
    // `*` silences every rule on that edge
    tester.pass(&["a.ts -> b.ts [ignore=*]", "b.ts -> a.ts [ignore=*]"]);
    // file-level suppression silences the module-level orphan rule
    tester.pass(&["lonely.ts [ignore-file=no-orphans]"]);
    tester.fail(&["lonely.ts [ignore-file=no-circular]"]);
    let r = tester.run(&[
        "a.ts -> b.ts [ignore=no-circular]",
        "b.ts -> a.ts [ignore=no-circular]",
    ]);
    assert_eq!(r.violations.len(), 0);
    assert_eq!(r.suppressed, 2);
}

#[test]
fn overrides_change_severity_and_options_per_file() {
    let tester = t(json!({
        "rules": { "no-circular": "error" },
        "overrides": [
            { "files": ["legacy/**"], "rules": { "no-circular": "warn" } },
            { "files": ["generated/**"], "rules": { "no-circular": "off" } }
        ]
    }));
    let out = tester.fail(&[
        "src/a.ts -> src/b.ts",
        "src/b.ts -> src/a.ts",
        "legacy/a.ts -> legacy/b.ts",
        "legacy/b.ts -> legacy/a.ts",
        "generated/a.ts -> generated/b.ts",
        "generated/b.ts -> generated/a.ts",
    ]);
    insta::assert_snapshot!("overrides", out);

    // an override can enable a rule that is off by default, with its own options
    let enable =
        t(json!({ "overrides": [{ "files": ["strict/**"], "rules": { "no-orphans": "error" } }] }));
    enable.pass(&["loose/lonely.ts"]);
    enable.fail(&["strict/lonely.ts"]);
}

#[test]
fn dead_rules_are_reported_when_asked() {
    let config = |report: bool| {
        json!({
            "options": { "reportDeadRules": report },
            "forbid": [
                { "name": "typo-path", "from": { "path": "scr/**" }, "to": { "path": "x.ts" } },
                { "name": "never-targets", "from": { "path": "src/**" }, "to": { "path": "nothing/**" } },
                { "name": "alive", "from": { "path": "src/**" }, "to": { "path": "src/b.ts" } },
            ]
        })
    };
    let tester = t(config(true));
    let r = tester.run(&["src/a.ts -> src/b.ts"]);
    insta::assert_snapshot!("dead_rules", r.warnings.join("\n"));
    assert!(
        t(config(false))
            .run(&["src/a.ts -> src/b.ts"])
            .warnings
            .is_empty()
    );
}

#[test]
fn bad_regex_names_the_rule() {
    let err = oxcruise_rules::Engine::new(
        &serde_json::from_value(json!({ "forbid": [{ "name": "lookahead", "from": { "path": { "regex": "^(?!src)" } } }] })).unwrap(),
        None,
    )
    .err()
    .unwrap();
    assert!(
        err[0].contains("lookahead") && err[0].contains("invalid pattern"),
        "{err:?}"
    );
}

#[test]
fn output_is_sorted_errors_first() {
    let tester = t(json!({ "rules": { "no-orphans": "warn", "no-unresolvable": "error" } }));
    let out = tester.fail(&["z.ts -> ./nope [unknown]", "a.ts"]);
    insta::assert_snapshot!("sorted", out);
}
