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
fn no_circular_ignore_type_only() {
    let strict = Tester::rule("no-circular", json!({ "ignoreTypeOnly": true }));
    strict.pass(&["a.ts -> b.ts", "b.ts -> a.ts [type-only]"]);
    // a runtime cycle is still reported, and the type-only edge that closes a bigger one is ignored
    insta::assert_snapshot!(
        "no_circular_ignore_type_only",
        strict.fail(&[
            "a.ts -> b.ts",
            "b.ts -> a.ts",
            "b.ts -> c.ts",
            "c.ts -> b.ts [type-only]"
        ])
    );
    // without the option the type-only cycle is reported (#695, #305)
    Tester::rule("no-circular", none()).fail(&["a.ts -> b.ts", "b.ts -> a.ts [type-only]"]);
}

#[test]
fn no_orphans() {
    let t = Tester::rule(
        "no-orphans",
        json!({ "ignore": [{ "regex": "[.]d[.]ts$" }, "**/*.config.ts"] }),
    );
    t.pass(&["a.ts -> b.ts", "types.d.ts", "vite.config.ts"]);
    insta::assert_snapshot!(
        "no_orphans",
        t.fail(&["a.ts -> b.ts", "lonely.ts", "dir/other.ts"])
    );
}

#[test]
fn dependency_type_rules() {
    Tester::rule("no-deprecated-core", none()).pass(&["a.ts -> fs [core]"]);
    insta::assert_snapshot!(
        "no_deprecated_core",
        Tester::rule("no-deprecated-core", none()).fail(&["a.ts -> sys [core deprecated]"])
    );

    Tester::rule("no-deprecated-npm", none()).pass(&["a.ts -> left-pad [npm]"]);
    insta::assert_snapshot!(
        "no_deprecated_npm",
        Tester::rule("no-deprecated-npm", none()).fail(&["a.ts -> left-pad [npm deprecated]"])
    );

    Tester::rule("no-non-package-json", none()).pass(&["a.ts -> x [npm]"]);
    insta::assert_snapshot!(
        "no_non_package_json",
        Tester::rule("no-non-package-json", none()).fail(&["a.ts -> x [npm-no-pkg]"])
    );

    Tester::rule("no-unresolvable", none()).pass(&["a.ts -> b.ts"]);
    insta::assert_snapshot!(
        "no_unresolvable",
        Tester::rule("no-unresolvable", none()).fail(&["a.ts -> ./nope [unknown]"])
    );

    let dup = Tester::rule("no-duplicate-dep-types", none());
    dup.pass(&["a.ts -> x [npm-dev]", "a.ts -> y [npm npm-dev type-only]"]);
    insta::assert_snapshot!(
        "no_duplicate_dep_types",
        dup.fail(&["a.ts -> x [npm npm-dev]"])
    );

    Tester::rule("optional-deps-used", none()).pass(&["a.ts -> x [npm]"]);
    insta::assert_snapshot!(
        "optional_deps_used",
        Tester::rule("optional-deps-used", none()).fail(&["a.ts -> x [npm-optional]"])
    );
    Tester::rule("peer-deps-used", none()).pass(&["a.ts -> x [npm]"]);
    insta::assert_snapshot!(
        "peer_deps_used",
        Tester::rule("peer-deps-used", none()).fail(&["a.ts -> x [npm-peer]"])
    );
}

#[test]
fn not_to_test_and_spec() {
    let t = Tester::rule("not-to-test", none());
    t.pass(&[
        "src/a.ts -> src/b.ts",
        "src/a.test.ts -> src/a.ts",
        "src/a.test.ts -> src/helpers.test.ts",
    ]);
    insta::assert_snapshot!(
        "not_to_test",
        t.fail(&["src/a.ts -> src/a.test.ts", "src/a.ts -> test/util.ts"])
    );
    let s = Tester::rule("not-to-spec", none());
    s.pass(&["src/a.ts -> src/a.test.ts"]);
    insta::assert_snapshot!("not_to_spec", s.fail(&["src/a.ts -> src/a.spec.ts"]));
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
    assert!(rules.len() >= 15);
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
            .map(|r| format!(
                "{} {} {}{}",
                r.name,
                r.category,
                r.version,
                if r.recommended { " recommended" } else { "" }
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
