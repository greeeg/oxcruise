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
            "to": ["src/features/**", "!src/features/{feature}/**"]
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
fn the_message_replaces_the_generic_text() {
    let tester = t(json!({
        "forbid": [{ "name": "no-bad", "message": "bad.ts is going away", "to": "bad.ts" }]
    }));
    let out = tester.fail(&["a.ts -> bad.ts"]);
    assert!(out.ends_with(": bad.ts is going away"), "{out}");
}

#[test]
fn a_list_of_only_negations_means_everything_except() {
    let tester = t(json!({
        "forbid": [{ "name": "prod-only", "from": ["!**/*.spec.ts"], "to": "lib/**" }]
    }));
    tester.pass(&["src/a.spec.ts -> lib/x.ts"]);
    tester.fail(&["src/a.ts -> lib/x.ts"]);

    // ... on the `to` side too: anything but the allowed folders
    let to = t(json!({
        "forbid": [{ "name": "only-shared", "from": "src/**", "to": ["!src/shared/**", "!lib/**"] }]
    }));
    to.pass(&["src/a.ts -> src/shared/x.ts", "src/a.ts -> lib/y.ts"]);
    to.fail(&["src/a.ts -> src/b.ts"]);
}

#[test]
fn relation_conditions() {
    let sib = t(json!({
        "forbid": [{ "name": "no-siblings", "to": { "relation": ["sibling", "same-package"] } }]
    }))
    .with_package("pkg", 0);
    // outside the package, in the same folder: neither relation holds
    sib.pass(&["a/x.ts -> a/y.ts"]);
    insta::assert_snapshot!(
        "relation_sibling",
        sib.fail(&["a/x.ts -> b/y.ts", "pkg/x.ts -> pkg/y.ts"])
    );

    // a single relation may be written without a list
    let one = t(json!({ "forbid": [{ "name": "r", "to": { "relation": "sibling" } }] }));
    one.pass(&["src/a/x.ts -> src/a/y.ts"]);
    one.fail(&["src/a/x.ts -> src/b/y.ts"]);
}

#[test]
fn kind_and_edge_conditions() {
    let tester = t(json!({
        "forbid": [
            { "name": "no-builtin-in-web", "from": "web/**", "to": { "kind": "builtin" } },
            { "name": "no-unresolved", "to": { "kind": "unresolved" } },
            { "name": "no-dynamic", "to": { "dynamic": true } },
            { "name": "no-type-only-packages", "to": { "kind": ["package"], "typeOnly": true } },
        ]
    }));
    tester.pass(&[
        "web/a.ts -> web/b.ts",
        "server/a.ts -> fs [core]",
        "a.ts -> x [npm]",
    ]);
    insta::assert_snapshot!(
        "kind_conditions",
        tester.fail(&[
            "web/a.ts -> fs [core]",
            "a.ts -> ./nope [unknown]",
            "a.ts -> b.ts [dynamic-import]",
            "a.ts -> types-pkg [npm type-only]",
        ])
    );
}

#[test]
fn kind_local_matches_project_files_only() {
    let tester = t(json!({
        "forbid": [{ "name": "no-local-from-scripts", "from": "scripts/**", "to": { "kind": "local" } }]
    }));
    tester.pass(&["scripts/a.ts -> fs [core]", "scripts/a.ts -> x [npm]"]);
    tester.fail(&["scripts/a.ts -> src/b.ts"]);
}

#[test]
fn reachable() {
    let tester = t(json!({
        "forbid": [{ "name": "ui-must-not-reach-db", "from": "src/ui/**", "to": { "path": "src/db/**", "reachable": true } }]
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

    // `kind` narrows what counts: here only packages
    let pkgs = t(json!({
        "forbid": [{ "name": "ui-no-pg", "from": "src/ui/**", "to": { "path": "pg", "kind": "package", "reachable": true } }]
    }));
    pkgs.pass(&["src/ui/a.ts -> src/db.ts"]);
    pkgs.fail(&["src/ui/a.ts -> src/db.ts", "src/db.ts -> pg [npm]"]);
}

#[test]
fn unsupported_combinations_are_errors() {
    for (to, needle) in [
        (json!({ "reachable": false }), "`reachable: false`"),
        (
            json!({ "reachable": true, "typeOnly": true }),
            "`reachable` can only",
        ),
    ] {
        let err = oxcruise_rules::Engine::new(
            &serde_json::from_value(json!({ "forbid": [{ "name": "r", "to": to }] })).unwrap(),
            None,
        )
        .err()
        .unwrap();
        assert!(err[0].contains("`r`") && err[0].contains(needle), "{err:?}");
    }
}

#[test]
fn a_forbid_rule_takes_its_level_from_severity_only() {
    let forbid = json!([{ "name": "r", "severity": "warn", "to": "bad.ts" }]);
    let out = t(json!({ "forbid": forbid })).fail(&["a.ts -> bad.ts"]);
    assert!(out.starts_with("warn r"), "{out}");

    // `rules` is for built-in and plugin rules; naming a forbid rule there is an error
    let err = oxcruise_rules::Engine::new(
        &serde_json::from_value(json!({ "forbid": forbid, "rules": { "r": "off" } })).unwrap(),
        None,
    )
    .err()
    .unwrap();
    assert!(err[0].contains("unknown rule `r`"), "{err:?}");

    // a forbid rule may not reuse a built-in name
    let err = oxcruise_rules::Engine::new(
        &serde_json::from_value(json!({ "forbid": [{ "name": "no-circular" }] })).unwrap(),
        None,
    )
    .err()
    .unwrap();
    assert!(err[0].contains("already used"), "{err:?}");
}

#[test]
fn overrides_can_change_a_forbid_rule_per_file() {
    let tester = t(json!({
        "forbid": [{ "name": "no-bad", "to": "bad.ts" }],
        "overrides": [
            { "files": ["scripts/**"], "rules": { "no-bad": "off" } },
            { "files": ["legacy/**"], "rules": { "no-bad": "warn" } },
        ]
    }));
    tester.pass(&["scripts/a.ts -> bad.ts"]);
    let out = tester.fail(&["src/a.ts -> bad.ts", "legacy/a.ts -> bad.ts"]);
    insta::assert_snapshot!("forbid_overrides", out);
}

#[test]
fn suppression_comments() {
    let tester = t(json!({ "rules": { "no-circular": "error", "no-orphans": "error" } }));
    // a cycle is reported once; annotating any one of its imports accepts the whole cycle
    tester.pass(&["a.ts -> b.ts [ignore=no-circular]", "b.ts -> a.ts"]);
    tester.pass(&["a.ts -> b.ts", "b.ts -> a.ts [ignore=no-circular]"]);
    // ... but an independent cycle next to it is still reported
    let out = tester.fail(&[
        "a.ts -> b.ts [ignore=no-circular]",
        "b.ts -> a.ts",
        "c.ts -> d.ts",
        "d.ts -> c.ts",
    ]);
    insta::assert_snapshot!("suppression_cycle", out);
    // a different rule name does not suppress it
    tester.fail(&["a.ts -> b.ts [ignore=no-orphans]", "b.ts -> a.ts"]);
    // `*` silences every rule on that edge
    tester.pass(&["a.ts -> b.ts [ignore=*]", "b.ts -> a.ts"]);
    // file-level suppression silences the module-level orphan rule
    tester.pass(&["lonely.ts [ignore-file=no-orphans]"]);
    tester.fail(&["lonely.ts [ignore-file=no-circular]"]);
    let r = tester.run(&["a.ts -> b.ts [ignore=no-circular]", "b.ts -> a.ts"]);
    assert_eq!(r.violations.len(), 0);
    assert_eq!(r.suppressed, 1, "one cycle, not one per edge");
}

#[test]
fn every_edge_of_a_cycle_is_one_violation_total() {
    let tester = t(json!({ "rules": { "no-circular": "error" } }));
    let r = tester.run(&["a.ts -> b.ts", "b.ts -> c.ts", "c.ts -> a.ts"]);
    assert_eq!(r.violations.len(), 1);
    assert_eq!(r.violations[0].path.len(), 3);
    // two cycles that share a module are two violations
    let two = tester.run(&[
        "a.ts -> b.ts",
        "b.ts -> a.ts",
        "a.ts -> c.ts",
        "c.ts -> a.ts",
    ]);
    assert_eq!(two.violations.len(), 2);
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

    // an override can enable a rule that is off, with its own options
    let enable =
        t(json!({ "overrides": [{ "files": ["strict/**"], "rules": { "no-orphans": "error" } }] }));
    enable.pass(&["loose/lonely.ts"]);
    enable.fail(&["strict/lonely.ts"]);
}

#[test]
fn dead_rules_are_always_reported() {
    let tester = t(json!({
        "forbid": [
            { "name": "typo-path", "from": "scr/**", "to": "x.ts" },
            { "name": "never-targets", "from": "src/**", "to": "nothing/**" },
            { "name": "alive", "from": "src/**", "to": "src/b.ts" },
        ]
    }));
    let r = tester.run(&["src/a.ts -> src/b.ts"]);
    insta::assert_snapshot!("dead_rules", r.warnings.join("\n"));
}

#[test]
fn bad_glob_names_the_rule() {
    let err = oxcruise_rules::Engine::new(
        &serde_json::from_value(json!({ "forbid": [{ "name": "broken", "from": "src/{a,/**" }] }))
            .unwrap(),
        None,
    );
    // an unterminated brace is taken literally, so the rule compiles; nothing may panic
    assert!(err.is_ok() || err.err().unwrap()[0].contains("broken"));
}

#[test]
fn output_is_sorted_errors_first() {
    let tester = t(json!({ "rules": { "no-orphans": "warn", "no-unresolvable": "error" } }));
    let out = tester.fail(&["z.ts -> ./nope [unknown]", "a.ts"]);
    insta::assert_snapshot!("sorted", out);
}
