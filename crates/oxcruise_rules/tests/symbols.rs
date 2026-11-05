//! `unused-exports` and `unused-packages`, on graphs with exports, used names and manifests.

use oxcruise_rules::testing::Tester;
use serde_json::{Value, json};

fn exports(options: Value) -> Tester {
    Tester::rule("unused-exports", options)
}

fn deps(options: Value) -> Tester {
    Tester::rule("unused-packages", options)
}

#[test]
fn unused_exports_are_reported_and_used_ones_are_not() {
    insta::assert_snapshot!(
        "unused_exports_basic",
        exports(Value::Null).fail(&[
            "main.ts -> lib.ts [names=used,Used]",
            "main.ts [exports=run]",
            "lib.ts [exports=used,unused,T:type,default]",
        ])
    );
}

#[test]
fn an_import_that_cannot_be_narrowed_uses_everything() {
    exports(Value::Null).pass(&[
        "main.ts -> lib.ts",
        "main.ts [exports=run]",
        "lib.ts [exports=a,b,default]",
    ]);
}

#[test]
fn exports_travel_through_barrels() {
    let t = exports(Value::Null);
    insta::assert_snapshot!(
        "unused_exports_barrel",
        t.fail(&[
            "app.ts -> index.ts [names=y]",
            "index.ts -> impl.ts [reexport=x:y names=]",
            "index.ts -> other.ts [star names=]",
            "impl.ts [exports=x,z]",
            "other.ts [exports=o]",
            "app.ts [exports=main]",
        ])
    );
    // `export * as ns` and `ns.a`
    insta::assert_snapshot!(
        "unused_exports_namespace",
        t.fail(&[
            "app.ts -> index.ts [names=ns.a]",
            "index.ts -> impl.ts [ns=ns names=]",
            "impl.ts [exports=a,b]",
            "app.ts [exports=main]",
        ])
    );
}

#[test]
fn entry_points_keep_their_exports() {
    // `api.ts` is imported inside the project, but declared public by an option
    let lines = [
        "app.ts -> api.ts [names=a]",
        "app.ts [exports=main]",
        "api.ts [exports=a,b]",
    ];
    insta::assert_snapshot!("unused_exports_inner", exports(Value::Null).fail(&lines));
    exports(json!({ "entries": ["api.ts"] })).pass(&lines);
    // checking entry points against their importers only
    insta::assert_snapshot!(
        "unused_exports_entry_exports",
        exports(json!({ "includeEntryExports": true })).fail(&lines)
    );
}

#[test]
fn options_narrow_the_report() {
    let lines = [
        "main.ts -> lib.ts [names=a]",
        "main.ts [exports=run]",
        "lib.ts [exports=a,T:type,inFile^,b]",
    ];
    insta::assert_snapshot!("unused_exports_default", exports(Value::Null).fail(&lines));
    insta::assert_snapshot!(
        "unused_exports_no_types",
        exports(json!({ "types": false })).fail(&lines)
    );
    insta::assert_snapshot!(
        "unused_exports_in_file",
        exports(json!({ "ignoreExportsUsedInFile": true, "types": false })).fail(&lines)
    );
    exports(json!({ "ignore": ["lib.ts"] })).pass(&lines);
}

#[test]
fn unused_members_of_a_used_enum_are_reported() {
    let t = exports(Value::Null);
    insta::assert_snapshot!(
        "unused_exports_members",
        t.fail(&[
            "main.ts -> lib.ts [names=E.A,Whole,Ns.f]",
            "main.ts [exports=run]",
            "lib.ts [exports=E{A+B+C},Whole{X+Y},Ns{f+g},Dead{Q}]",
        ])
    );
    exports(json!({ "members": false })).fail(&[
        "main.ts -> lib.ts [names=E.A]",
        "main.ts [exports=run]",
        "lib.ts [exports=E{A+B},Other]",
    ]);
}

#[test]
fn a_comment_silences_one_export() {
    insta::assert_snapshot!(
        "unused_exports_suppressed",
        exports(Value::Null).fail(&[
            "main.ts -> lib.ts [names=a]",
            "main.ts [exports=run]",
            "lib.ts [exports=a,quiet~unused-exports,loud~other-rule,all~*]",
        ])
    );
}

#[test]
fn one_binding_exported_twice_is_reported() {
    insta::assert_snapshot!(
        "unused_exports_duplicates",
        exports(Value::Null).fail(&[
            "main.ts -> lib.ts [names=Foo,default]",
            "main.ts [exports=run]",
            "lib.ts [exports=default,Foo@default]",
        ])
    );
}

#[test]
fn unused_packages_are_reported_at_their_line_in_package_json() {
    let t = deps(Value::Null).with_manifest(
        "",
        &[
            ("dependencies", "used"),
            ("dependencies", "unused"),
            ("dependencies", "@scope/pkg"),
            ("devDependencies", "tool"),
            ("peerDependencies", "peer"),
        ],
    );
    insta::assert_snapshot!(
        "unused_packages_basic",
        t.fail(&[
            "src/a.ts -> used/sub/path as used/sub/path [npm]",
            "src/a.ts -> @scope/pkg/x as @scope/pkg/x [npm]",
        ])
    );
}

#[test]
fn types_node_and_peers_follow_what_is_used() {
    let t = deps(json!({ "sections": ["dependencies", "devDependencies", "peerDependencies"] }))
        .with_manifest(
            "",
            &[
                ("dependencies", "react-dom>react"),
                ("peerDependencies", "react"),
                ("devDependencies", "@types/node"),
                ("devDependencies", "@types/lodash"),
                ("dependencies", "lodash"),
                ("devDependencies", "@types/unrelated"),
                ("devDependencies", "@types/scope__pkg"),
                ("dependencies", "@scope/pkg"),
            ],
        );
    insta::assert_snapshot!(
        "unused_packages_follow",
        t.fail(&[
            "src/a.ts -> react-dom [npm]",
            "src/a.ts -> lodash [npm]",
            "src/a.ts -> fs [core]",
            "src/a.ts -> @scope/pkg [npm]",
        ])
    );
}

#[test]
fn each_package_json_checks_its_own_modules() {
    let t = deps(Value::Null)
        .with_manifest(
            "",
            &[("dependencies", "root-only"), ("dependencies", "shared")],
        )
        .with_manifest(
            "packages/a",
            &[("dependencies", "shared"), ("dependencies", "a-only")],
        )
        .with_manifest("packages/empty", &[("dependencies", "never-checked")]);
    insta::assert_snapshot!(
        "unused_packages_workspaces",
        t.fail(&[
            "src/x.ts -> root-only [npm]",
            "packages/a/src/y.ts -> shared [npm]",
            "packages/a/src/y.ts -> ./z.ts",
        ])
    );
}

#[test]
fn whole_graph_rules_survive_overrides() {
    let lines = [
        "packages/a/src/x.ts -> ./y.ts",
        "packages/b/src/x.ts -> ./y.ts",
    ];
    let with = |overrides: Value| {
        Tester::config(json!({
            "rules": { "unused-packages": "error" },
            "overrides": overrides,
        }))
        .with_manifest("packages/a", &[("dependencies", "a-dep")])
        .with_manifest("packages/b", &[("dependencies", "b-dep")])
    };
    let count = |t: &Tester| t.run(&lines).violations.len();

    // an override that matches nothing must not hide the rule
    assert_eq!(
        count(&with(
            json!([{ "files": ["nothing/**"], "rules": { "unused-packages": "warn" } }])
        )),
        2
    );
    // turning it off for one package hides only that package's finding
    let t = with(json!([{ "files": ["packages/a/**"], "rules": { "unused-packages": "off" } }]));
    let result = t.run(&lines);
    assert_eq!(result.violations.len(), 1);
    assert!(
        result.violations[0].message.contains("b-dep"),
        "{:?}",
        result.violations
    );
}

#[test]
fn unused_packages_options() {
    let lines = ["src/a.ts -> used [npm]"];
    let manifest = [
        ("dependencies", "used"),
        ("dependencies", "ignored-one"),
        ("devDependencies", "dev"),
    ];
    deps(json!({ "ignore": ["ignored-*"] }))
        .with_manifest("", &manifest)
        .pass(&lines);
    insta::assert_snapshot!(
        "unused_packages_sections",
        deps(json!({ "sections": ["devDependencies"] }))
            .with_manifest("", &manifest)
            .fail(&lines)
    );
}

fn files(options: Value) -> Tester {
    Tester::rule("unused-modules", options)
}

#[test]
fn files_not_reachable_from_an_entry_are_reported() {
    let t = files(Value::Null)
        .with_manifest("", &[("dependencies", "x")])
        .with_entry_files("", &["dist/cli.js"], (Some("dist"), Some("src")));
    insta::assert_snapshot!(
        "unused_modules_basic",
        t.fail(&[
            "src/cli.ts -> src/used.ts",
            "src/used.ts -> src/deeper.ts",
            "src/dead.ts -> src/dead2.ts",
            "src/dead2.ts -> src/dead.ts",
            "src/types.d.ts",
            "vite.config.ts",
        ])
    );
}

#[test]
fn without_entries_nothing_is_reported() {
    files(Value::Null)
        .with_manifest("", &[])
        .pass(&["src/a.ts -> src/b.ts", "src/c.ts"]);
}

#[test]
fn index_files_and_the_entries_option_are_entries() {
    let t = files(json!({ "entries": ["scripts/*.ts"] })).with_manifest("", &[]);
    insta::assert_snapshot!(
        "unused_modules_entries",
        t.fail(&[
            "src/index.ts -> src/a.ts",
            "scripts/build.ts -> scripts/lib.ts",
            "src/orphan.ts",
        ])
    );
}

#[test]
fn ignore_file_keeps_a_module_and_what_it_imports() {
    let t = files(Value::Null).with_manifest("", &[]);
    t.pass(&[
        "src/index.ts",
        "src/kept.ts -> src/dep.ts [ignore-file=unused-modules]",
        "src/kept.ts [ignore-file=unused-modules]",
    ]);
}

#[test]
fn tool_conventions_make_entries() {
    let manifest = [("devDependencies", "vitest"), ("dependencies", "next")];
    let lines = [
        "app/page.tsx -> components/button.tsx",
        "app/helpers.ts",
        "src/x.test.ts -> src/x.ts",
        "next.config.ts",
        "src/unused.ts",
    ];
    insta::assert_snapshot!(
        "unused_modules_conventions",
        files(Value::Null).with_manifest("", &manifest).fail(&lines)
    );
    insta::assert_snapshot!(
        "unused_modules_conventions_off",
        files(json!({ "conventions": ["vitest"] }))
            .with_manifest("", &manifest)
            .fail(&lines)
    );
}

#[test]
fn conventions_make_exports_public_and_packages_used() {
    // an export of a route file is public, and `@vitest/coverage-v8` is loaded by vitest
    exports(Value::Null)
        .with_manifest("", &[("devDependencies", "next")])
        .pass(&[
            "app/page.tsx -> lib.ts [names=a]",
            "app/page.tsx [exports=default]",
            "lib.ts [exports=a]",
        ]);
    deps(json!({ "sections": ["devDependencies"] }))
        .with_manifest(
            "",
            &[
                ("devDependencies", "vitest"),
                ("devDependencies", "@vitest/coverage-v8"),
            ],
        )
        .pass(&["src/a.ts -> vitest [npm-dev]"]);
    insta::assert_snapshot!(
        "unused_packages_conventions_off",
        deps(json!({ "sections": ["devDependencies"], "conventions": false }))
            .with_manifest(
                "",
                &[
                    ("devDependencies", "vitest"),
                    ("devDependencies", "@vitest/coverage-v8"),
                ],
            )
            .fail(&["src/a.ts -> vitest [npm-dev]"])
    );
}

#[test]
fn unknown_conventions_are_rejected() {
    let cfg: oxcruise_config::OxcruiseConfig = serde_json::from_value(json!({
        "rules": { "unused-modules": ["error", { "conventions": ["nope"] }] }
    }))
    .unwrap();
    let err = oxcruise_rules::Engine::new(&cfg, None).err().unwrap();
    assert!(
        err.iter().any(|e| e.contains("unknown convention `nope`")),
        "{err:?}"
    );
}

#[test]
fn production_mode_leaves_tests_out() {
    let lines = [
        "src/a.ts -> src/lib.ts [names=used]",
        "src/a.ts [exports=run]",
        "src/lib.test.ts -> src/lib.ts [names=testOnly]",
        "src/lib.test.ts [exports=helper]",
        "src/lib.ts [exports=used,testOnly]",
    ];
    exports(Value::Null).pass(&lines);
    insta::assert_snapshot!(
        "unused_exports_production",
        exports(json!({ "production": true })).fail(&lines)
    );

    // files only tests reach, and dev dependencies
    let t = files(json!({ "production": true })).with_manifest("", &[]);
    insta::assert_snapshot!(
        "unused_modules_production",
        t.fail(&[
            "src/index.ts -> src/a.ts",
            "src/lib.test.ts -> src/helper.ts",
        ])
    );
    deps(json!({ "production": true, "sections": ["dependencies", "devDependencies"] }))
        .with_manifest("", &[("devDependencies", "tool"), ("dependencies", "used")])
        .pass(&["src/a.ts -> used [npm]", "src/a.test.ts -> tool [npm-dev]"]);
    insta::assert_snapshot!(
        "unused_packages_production",
        deps(json!({ "production": true }))
            .with_manifest("", &[("dependencies", "only-tests")])
            .fail(&["src/a.test.ts -> only-tests [npm]", "src/a.ts"])
    );
}

#[test]
fn set_option_overrides_the_config() {
    let cfg: oxcruise_config::OxcruiseConfig = serde_json::from_value(json!({
        "categories": { "correctness": "off", "suspicious": "off" },
        "rules": { "unused-exports": ["error", { "types": false }] }
    }))
    .unwrap();
    let mut engine = oxcruise_rules::Engine::new(&cfg, None).unwrap();
    engine.set_option("unused-exports", "production", json!(true));
    engine.set_option("unused-packages", "production", json!(true));
    assert!(engine.wants_symbols());
}
