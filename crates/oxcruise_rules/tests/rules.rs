use oxcruise_rules::testing::Tester;
use serde_json::{Value, json};

fn none() -> Value {
    Value::Null
}

#[test]
fn no_circular() {
    let t = Tester::rule("no-circular", none());
    t.pass(&["a.ts -> b.ts", "b.ts -> c.ts"]);
    insta::assert_snapshot!(
        "no_circular_basic",
        t.fail(&[
            "a.ts -> b.ts",
            "b.ts -> c.ts",
            "c.ts -> a.ts",
            "c.ts -> d.ts"
        ])
    );
    insta::assert_snapshot!("no_circular_self", t.fail(&["a.ts -> a.ts"]));

    // shortest cycle through an edge is reported, not an arbitrary one
    insta::assert_snapshot!(
        "no_circular_shortest",
        t.fail(&[
            "a.ts -> b.ts",
            "b.ts -> c.ts",
            "c.ts -> d.ts",
            "d.ts -> a.ts",
            "b.ts -> a.ts"
        ])
    );
}

#[test]
fn no_circular_ignores_type_only_cycles_by_default() {
    let default = Tester::rule("no-circular", none());
    default.pass(&["a.ts -> b.ts", "b.ts -> a.ts [type-only]"]);
    // a runtime cycle is still reported, and the type-only edge that closes a bigger one is ignored
    insta::assert_snapshot!(
        "no_circular_ignore_type_only",
        default.fail(&[
            "a.ts -> b.ts",
            "b.ts -> a.ts",
            "b.ts -> c.ts",
            "c.ts -> b.ts [type-only]"
        ])
    );
    // with the option the type-only cycle is reported (#695, #305)
    Tester::rule("no-circular", json!({ "includeTypeOnly": true }))
        .fail(&["a.ts -> b.ts", "b.ts -> a.ts [type-only]"]);
}

#[test]
fn no_circular_ignore_removes_modules_before_searching() {
    let cycle = [
        "a.ts -> index.ts",
        "index.ts -> a.ts",
        "b.ts -> index.ts",
        "index.ts -> b.ts",
    ];
    Tester::rule("no-circular", none()).fail(&cycle);
    // a barrel everything goes through is not part of any cycle
    Tester::rule("no-circular", json!({ "ignore": ["index.ts"] })).pass(&cycle);
    // other cycles are still found
    insta::assert_snapshot!(
        "no_circular_ignore",
        Tester::rule("no-circular", json!({ "ignore": ["index.ts"] })).fail(&[
            "a.ts -> index.ts",
            "index.ts -> a.ts",
            "x.ts -> y.ts",
            "y.ts -> x.ts",
        ])
    );
}

#[test]
fn no_circular_caps_searches_per_component() {
    // a ring of five plus an unrelated two-cycle
    let lines = [
        "r0.ts -> r1.ts",
        "r1.ts -> r2.ts",
        "r2.ts -> r3.ts",
        "r3.ts -> r4.ts",
        "r4.ts -> r0.ts",
        "x.ts -> y.ts",
        "y.ts -> x.ts",
    ];
    let capped = Tester::rule("no-circular", json!({ "maxCyclesPerComponent": 2 }));
    let r = capped.run(&lines);
    let messages: Vec<_> = r.violations.iter().map(|v| v.message.as_str()).collect();
    // the ring is one cycle, found from its first edges; the summary names what was left out
    assert!(
        messages
            .iter()
            .any(|m| m.contains("3 more dependencies in this group of 5 modules")),
        "{messages:?}"
    );
    // the other component is under the limit and untouched
    assert!(messages.iter().any(|m| m.contains("x.ts → y.ts → x.ts")));
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.contains("more dependencies"))
            .count(),
        1
    );
    insta::assert_snapshot!("no_circular_capped", capped.fail(&lines));

    // the default limit and `0` leave small cycles alone
    let r = Tester::rule("no-circular", none()).run(&lines);
    assert!(
        r.violations
            .iter()
            .all(|v| !v.message.contains("more dependencies"))
    );
    let r = Tester::rule("no-circular", json!({ "maxCyclesPerComponent": 0 })).run(&lines);
    assert!(
        r.violations
            .iter()
            .all(|v| !v.message.contains("more dependencies"))
    );
}

#[test]
fn no_orphans() {
    // the defaults cover declarations and tool configuration
    let t = Tester::rule("no-orphans", none());
    t.pass(&[
        "a.ts -> b.ts",
        "types.d.ts",
        "vite.config.ts",
        "eslint.config.mjs",
        ".eslintrc.js",
        "sub/tailwind.config.js",
    ]);
    insta::assert_snapshot!(
        "no_orphans",
        t.fail(&["a.ts -> b.ts", "lonely.ts", "dir/other.ts"])
    );

    // `ignore` extends the defaults instead of replacing them
    let t = Tester::rule("no-orphans", json!({ "ignore": ["**/*.stories.ts"] }));
    t.pass(&["a.stories.ts", "types.d.ts"]);
    t.fail(&["lonely.ts"]);
}

#[test]
fn dependency_type_rules() {
    let deprecated = Tester::rule("no-deprecated", none());
    deprecated.pass(&["a.ts -> fs [core]", "a.ts -> left-pad [npm]"]);
    insta::assert_snapshot!(
        "no_deprecated",
        deprecated.fail(&[
            "a.ts -> sys [core deprecated]",
            "a.ts -> left-pad [npm deprecated]"
        ])
    );

    Tester::rule("no-undeclared-deps", none()).pass(&["a.ts -> x [npm]"]);
    insta::assert_snapshot!(
        "no_undeclared_deps",
        Tester::rule("no-undeclared-deps", none()).fail(&["a.ts -> x [npm-no-pkg]"])
    );

    Tester::rule("no-unresolvable", none()).pass(&["a.ts -> b.ts"]);
    insta::assert_snapshot!(
        "no_unresolvable",
        Tester::rule("no-unresolvable", none()).fail(&["a.ts -> ./nope [unknown]"])
    );

    let dup = Tester::rule("no-duplicate-deps", none());
    dup.pass(&["a.ts -> x [npm-dev]", "a.ts -> y [npm npm-dev type-only]"]);
    insta::assert_snapshot!("no_duplicate_deps", dup.fail(&["a.ts -> x [npm npm-dev]"]));

    Tester::rule("no-optional-deps", none()).pass(&["a.ts -> x [npm]"]);
    insta::assert_snapshot!(
        "no_optional_deps",
        Tester::rule("no-optional-deps", none()).fail(&["a.ts -> x [npm-optional]"])
    );
    Tester::rule("no-peer-deps", none()).pass(&["a.ts -> x [npm]"]);
    insta::assert_snapshot!(
        "no_peer_deps",
        Tester::rule("no-peer-deps", none()).fail(&["a.ts -> x [npm-peer]"])
    );
}

#[test]
fn no_unresolvable_ignore() {
    let t = Tester::rule(
        "no-unresolvable",
        json!({ "ignore": ["virtual:*", "src/generated/**"] }),
    );
    t.pass(&[
        "a.ts -> virtual:icons [unknown]",
        "src/generated/api.ts -> ./missing [unknown]",
    ]);
    t.fail(&["a.ts -> ./missing [unknown]"]);
}

#[test]
fn no_test_imports() {
    let t = Tester::rule("no-test-imports", none());
    t.pass(&[
        "src/a.ts -> src/b.ts",
        "src/a.test.ts -> src/a.ts",
        "src/a.test.ts -> src/helpers.test.ts",
        "src/a.spec.ts -> src/helpers.test.ts",
    ]);
    insta::assert_snapshot!(
        "no_test_imports",
        t.fail(&[
            "src/a.ts -> src/a.test.ts",
            "src/a.ts -> test/util.ts",
            "src/a.ts -> src/a.spec.ts"
        ])
    );
}

#[test]
fn no_dev_deps_in_prod() {
    let t = Tester::rule("no-dev-deps-in-prod", none());
    t.pass(&[
        "src/a.ts -> vitest [npm-dev type-only]",
        "src/a.test.ts -> vitest [npm-dev]",
        "scripts/build.ts -> esbuild [npm-dev]",
        "src/b.ts -> both [npm npm-dev]",
    ]);
    insta::assert_snapshot!(
        "no_dev_deps_in_prod",
        t.fail(&[
            "src/a.ts -> vitest [npm-dev]",
            "packages/x/src/b.ts -> vite [npm-dev]"
        ])
    );

    let custom = Tester::rule(
        "no-dev-deps-in-prod",
        json!({ "prod": ["scripts/**"], "test": ["scripts/ok/**"] }),
    );
    custom.pass(&[
        "src/a.ts -> vitest [npm-dev]",
        "scripts/ok/x.ts -> vitest [npm-dev]",
    ]);
    custom.fail(&["scripts/x.ts -> vitest [npm-dev]"]);
}

#[test]
fn layers() {
    let t = Tester::rule(
        "layers",
        json!({
            "layers": { "ui": "src/ui/**", "domain": "src/domain/**", "infra": "src/infra/**" },
            "allow": { "ui": ["domain"], "infra": ["domain"], "domain": [] }
        }),
    );
    t.pass(&[
        "src/ui/a.ts -> src/domain/b.ts",
        "src/domain/b.ts -> src/domain/c.ts",
        "src/ui/a.ts -> src/other/x.ts",
    ]);
    insta::assert_snapshot!(
        "layers",
        t.fail(&[
            "src/domain/b.ts -> src/infra/db.ts",
            "src/ui/a.ts -> src/infra/db.ts",
            "src/ui/a.ts -> src/domain/b.ts"
        ])
    );
}

#[test]
fn layers_reject_unknown_names() {
    let cfg: oxcruise_config::OxcruiseConfig = serde_json::from_value(
        json!({ "rules": { "layers": ["error", { "layers": { "ui": "ui/**" }, "allow": { "ui": ["nope"] } }] } }),
    )
    .unwrap();
    let err = oxcruise_rules::Engine::new(&cfg, None).err().unwrap();
    assert!(err[0].contains("unknown layer `nope`"), "{err:?}");
}

#[test]
fn internal_modules() {
    let t = Tester::rule("internal-modules", none());
    t.pass(&[
        "src/a/x.ts -> src/a/internal/y.ts",
        "src/a/internal/y.ts -> src/a/internal/z.ts",
        "src/a/sub/x.ts -> src/a/internal/y.ts",
        "x.ts -> internal/top.ts",
        "src/b.ts -> src/internal.ts",
    ]);
    insta::assert_snapshot!(
        "internal_modules",
        t.fail(&[
            "src/b/x.ts -> src/a/internal/y.ts",
            "src/a2/x.ts -> src/a/internal/deep/y.ts"
        ])
    );
    Tester::rule("internal-modules", json!({ "marker": "private" }))
        .fail(&["b/x.ts -> a/private/y.ts"]);
}

#[test]
fn no_cross_package_relative() {
    let t = Tester::rule("no-cross-package-relative", none())
        .with_package("packages/a", 0)
        .with_package("packages/b", 1);
    t.pass(&[
        "packages/a/x.ts -> packages/a/y.ts as ./y",
        "packages/a/x.ts -> packages/b/src/y.ts as @acme/b",
    ]);
    insta::assert_snapshot!(
        "no_cross_package_relative",
        t.fail(&["packages/a/x.ts -> packages/b/src/y.ts as ../b/src/y"])
    );
}

#[test]
fn unknown_rule_suggests_a_name() {
    let cfg: oxcruise_config::OxcruiseConfig =
        serde_json::from_value(json!({ "rules": { "no-circluar": "error", "acme/x": "warn" } }))
            .unwrap();
    let err = oxcruise_rules::Engine::new(&cfg, None).err().unwrap();
    assert!(
        err.iter().any(|e| e.contains("did you mean `no-circular`")),
        "{err:?}"
    );
    assert!(
        err.iter().any(|e| e.contains("plugin rules need")),
        "{err:?}"
    );
}

#[test]
fn every_builtin_documents_itself() {
    let rules = oxcruise_rules::builtin_rules();
    assert_eq!(rules.len(), 13);
    for r in &rules {
        assert!(r.docs.len() > 20, "{} has no docs", r.name);
        assert!(
            r.name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "{}",
            r.name
        );
    }
    assert!(rules.windows(2).all(|w| w[0].name < w[1].name));
    insta::assert_snapshot!(
        "rule_list",
        rules
            .iter()
            .map(|r| format!("{} {} {}", r.name, r.category.as_str(), r.version))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
