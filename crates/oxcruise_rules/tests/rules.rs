use oxcruise_rules::testing::Tester;
use serde_json::{Value, json};

fn none() -> Value {
    Value::Null
}

#[test]
fn circular_dependencies() {
    let t = Tester::rule("circular-dependencies", none());
    t.pass(&["a.ts -> b.ts", "b.ts -> c.ts"]);
    insta::assert_snapshot!(
        "circular_dependencies_basic",
        t.fail(&[
            "a.ts -> b.ts",
            "b.ts -> c.ts",
            "c.ts -> a.ts",
            "c.ts -> d.ts"
        ])
    );
    insta::assert_snapshot!("circular_dependencies_self", t.fail(&["a.ts -> a.ts"]));

    // shortest cycle through an edge is reported, not an arbitrary one
    insta::assert_snapshot!(
        "circular_dependencies_shortest",
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
fn circular_dependencies_ignores_type_only_cycles_by_default() {
    let default = Tester::rule("circular-dependencies", none());
    default.pass(&["a.ts -> b.ts", "b.ts -> a.ts [type-only]"]);
    // a runtime cycle is still reported, and the type-only edge that closes a bigger one is ignored
    insta::assert_snapshot!(
        "circular_dependencies_ignore_type_only",
        default.fail(&[
            "a.ts -> b.ts",
            "b.ts -> a.ts",
            "b.ts -> c.ts",
            "c.ts -> b.ts [type-only]"
        ])
    );
    // with the option the type-only cycle is reported (#695, #305)
    Tester::rule("circular-dependencies", json!({ "includeTypeOnly": true }))
        .fail(&["a.ts -> b.ts", "b.ts -> a.ts [type-only]"]);
}

#[test]
fn circular_dependencies_ignore_removes_modules_before_searching() {
    let cycle = [
        "a.ts -> index.ts",
        "index.ts -> a.ts",
        "b.ts -> index.ts",
        "index.ts -> b.ts",
    ];
    Tester::rule("circular-dependencies", none()).fail(&cycle);
    // a barrel everything goes through is not part of any cycle
    Tester::rule("circular-dependencies", json!({ "ignore": ["index.ts"] })).pass(&cycle);
    // other cycles are still found
    insta::assert_snapshot!(
        "circular_dependencies_ignore",
        Tester::rule("circular-dependencies", json!({ "ignore": ["index.ts"] })).fail(&[
            "a.ts -> index.ts",
            "index.ts -> a.ts",
            "x.ts -> y.ts",
            "y.ts -> x.ts",
        ])
    );
}

#[test]
fn circular_dependencies_caps_searches_per_component() {
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
    let capped = Tester::rule(
        "circular-dependencies",
        json!({ "maxCyclesPerComponent": 2 }),
    );
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
    insta::assert_snapshot!("circular_dependencies_capped", capped.fail(&lines));

    // the default limit and `0` leave small cycles alone
    let r = Tester::rule("circular-dependencies", none()).run(&lines);
    assert!(
        r.violations
            .iter()
            .all(|v| !v.message.contains("more dependencies"))
    );
    let r = Tester::rule(
        "circular-dependencies",
        json!({ "maxCyclesPerComponent": 0 }),
    )
    .run(&lines);
    assert!(
        r.violations
            .iter()
            .all(|v| !v.message.contains("more dependencies"))
    );
}

#[test]
fn orphan_modules() {
    // the defaults cover declarations and tool configuration
    let t = Tester::rule("orphan-modules", none());
    t.pass(&[
        "a.ts -> b.ts",
        "types.d.ts",
        "vite.config.ts",
        "eslint.config.mjs",
        ".eslintrc.js",
        "sub/tailwind.config.js",
    ]);
    insta::assert_snapshot!(
        "orphan_modules",
        t.fail(&["a.ts -> b.ts", "lonely.ts", "dir/other.ts"])
    );

    // `ignore` extends the defaults instead of replacing them
    let t = Tester::rule("orphan-modules", json!({ "ignore": ["**/*.stories.ts"] }));
    t.pass(&["a.stories.ts", "types.d.ts"]);
    t.fail(&["lonely.ts"]);
}

#[test]
fn dependency_type_rules() {
    let deprecated = Tester::rule("deprecated-dependencies", none());
    deprecated.pass(&["a.ts -> fs [core]", "a.ts -> left-pad [npm]"]);
    insta::assert_snapshot!(
        "deprecated_dependencies",
        deprecated.fail(&[
            "a.ts -> sys [core deprecated]",
            "a.ts -> left-pad [npm deprecated]"
        ])
    );

    Tester::rule("undeclared-packages", none()).pass(&["a.ts -> x [npm]"]);
    insta::assert_snapshot!(
        "undeclared_packages",
        Tester::rule("undeclared-packages", none()).fail(&["a.ts -> x [npm-no-pkg]"])
    );

    Tester::rule("unresolved-dependencies", none()).pass(&["a.ts -> b.ts"]);
    insta::assert_snapshot!(
        "unresolved_dependencies",
        Tester::rule("unresolved-dependencies", none()).fail(&["a.ts -> ./nope [unknown]"])
    );

    let dup = Tester::rule("duplicate-packages", none());
    dup.pass(&["a.ts -> x [npm-dev]", "a.ts -> y [npm npm-dev type-only]"]);
    insta::assert_snapshot!("duplicate_packages", dup.fail(&["a.ts -> x [npm npm-dev]"]));

    Tester::rule("optional-dependencies", none()).pass(&["a.ts -> x [npm]"]);
    insta::assert_snapshot!(
        "optional_dependencies",
        Tester::rule("optional-dependencies", none()).fail(&["a.ts -> x [npm-optional]"])
    );
    Tester::rule("peer-dependencies", none()).pass(&["a.ts -> x [npm]"]);
    insta::assert_snapshot!(
        "peer_dependencies",
        Tester::rule("peer-dependencies", none()).fail(&["a.ts -> x [npm-peer]"])
    );
}

#[test]
fn unresolved_dependencies_ignore() {
    let t = Tester::rule(
        "unresolved-dependencies",
        json!({ "ignore": ["virtual:*", "src/generated/**"] }),
    );
    t.pass(&[
        "a.ts -> virtual:icons [unknown]",
        "src/generated/api.ts -> ./missing [unknown]",
    ]);
    t.fail(&["a.ts -> ./missing [unknown]"]);
}

#[test]
fn dependencies_on_tests() {
    let t = Tester::rule("dependencies-on-tests", none());
    t.pass(&[
        "src/a.ts -> src/b.ts",
        "src/a.test.ts -> src/a.ts",
        "src/a.test.ts -> src/helpers.test.ts",
        "src/a.spec.ts -> src/helpers.test.ts",
    ]);
    insta::assert_snapshot!(
        "dependencies_on_tests",
        t.fail(&[
            "src/a.ts -> src/a.test.ts",
            "src/a.ts -> test/util.ts",
            "src/a.ts -> src/a.spec.ts"
        ])
    );
}

#[test]
fn dev_dependencies_in_production() {
    let t = Tester::rule("dev-dependencies-in-production", none());
    t.pass(&[
        "src/a.ts -> vitest [npm-dev type-only]",
        "src/a.test.ts -> vitest [npm-dev]",
        "scripts/build.ts -> esbuild [npm-dev]",
        "src/b.ts -> both [npm npm-dev]",
    ]);
    insta::assert_snapshot!(
        "dev_dependencies_in_production",
        t.fail(&[
            "src/a.ts -> vitest [npm-dev]",
            "packages/x/src/b.ts -> vite [npm-dev]"
        ])
    );

    let custom = Tester::rule(
        "dev-dependencies-in-production",
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
        json!({ "rules": { "layers": { "level": "error", "layers": { "ui": "ui/**" }, "allow": { "ui": ["nope"] } } } }),
    )
    .unwrap();
    let err = oxcruise_rules::Engine::new(&cfg, None).err().unwrap();
    assert!(err[0].contains("unknown layer `nope`"), "{err:?}");
}

#[test]
fn visibility() {
    let t = Tester::rule("visibility", none());
    t.pass(&[
        "src/a/x.ts -> src/a/internal/y.ts",
        "src/a/internal/y.ts -> src/a/internal/z.ts",
        "src/a/sub/x.ts -> src/a/internal/y.ts",
        "x.ts -> internal/top.ts",
        "src/b.ts -> src/internal.ts",
    ]);
    insta::assert_snapshot!(
        "visibility",
        t.fail(&[
            "src/b/x.ts -> src/a/internal/y.ts",
            "src/a2/x.ts -> src/a/internal/deep/y.ts"
        ])
    );
    Tester::rule("visibility", json!({ "marker": "private" })).fail(&["b/x.ts -> a/private/y.ts"]);
}

#[test]
fn cross_package_relative_dependencies() {
    let t = Tester::rule("cross-package-relative-dependencies", none())
        .with_package("packages/a", 0)
        .with_package("packages/b", 1);
    t.pass(&[
        "packages/a/x.ts -> packages/a/y.ts as ./y",
        "packages/a/x.ts -> packages/b/src/y.ts as @acme/b",
    ]);
    insta::assert_snapshot!(
        "cross_package_relative_dependencies",
        t.fail(&["packages/a/x.ts -> packages/b/src/y.ts as ../b/src/y"])
    );
}

#[test]
fn unknown_rule_suggests_a_name() {
    let cfg: oxcruise_config::OxcruiseConfig = serde_json::from_value(
        json!({ "rules": { "circular-dependancies": "error", "acme/x": "warn" } }),
    )
    .unwrap();
    let err = oxcruise_rules::Engine::new(&cfg, None).err().unwrap();
    assert!(
        err.iter()
            .any(|e| e.contains("did you mean `circular-dependencies`")),
        "{err:?}"
    );
    assert!(
        err.iter().any(|e| e.contains("plugin rules need")),
        "{err:?}"
    );
}

#[test]
fn old_rule_names_say_they_were_renamed() {
    for (old, new) in oxcruise_rules::registry::RENAMED {
        let cfg: oxcruise_config::OxcruiseConfig =
            serde_json::from_value(json!({ "rules": { *old: "error" } })).unwrap();
        let err = oxcruise_rules::Engine::new(&cfg, None).err().unwrap();
        assert!(
            err.iter()
                .any(|e| e.contains(&format!("`{old}`; it was renamed to `{new}`"))),
            "{old}: {err:?}"
        );
        let cfg: oxcruise_config::OxcruiseConfig = serde_json::from_value(json!({
            "overrides": [{ "files": "src/**", "rules": { *old: "off" } }]
        }))
        .unwrap();
        let err = oxcruise_rules::Engine::new(&cfg, None).err().unwrap();
        assert!(
            err.iter().any(|e| e.contains("it was renamed to")),
            "{old}: {err:?}"
        );
    }
}

#[cfg(feature = "ruledocs")]
#[test]
fn no_rule_has_an_option_named_level() {
    for (name, schema) in oxcruise_rules::registry::builtin_option_schemas() {
        let has = schema
            .as_ref()
            .and_then(|s| s["properties"].as_object())
            .is_some_and(|p| p.contains_key("level"));
        assert!(!has, "`{name}` has an option `level`, which is reserved");
    }
}

#[cfg(feature = "ruledocs")]
#[test]
fn the_config_schema_types_the_options_of_each_rule() {
    let schema = oxcruise_rules::schema::config_json_schema();
    let rules = &schema["$defs"]["Rules"];
    let names: Vec<&str> = oxcruise_rules::builtin_rules()
        .iter()
        .map(|r| r.name)
        .collect();
    for name in &names {
        let entry = &rules["properties"][*name]["anyOf"];
        assert_eq!(entry[0]["$ref"], "#/$defs/Level", "{name}");
        assert_eq!(
            entry[1]["properties"]["level"]["$ref"], "#/$defs/Level",
            "{name}"
        );
    }
    // a rule with options lists them next to `level`, and rejects the rest
    let layers = &rules["properties"]["layers"]["anyOf"][1];
    assert!(layers["properties"]["allow"].is_object());
    assert_eq!(layers["additionalProperties"], false);
    // plugin rules keep the generic entry
    assert_eq!(rules["additionalProperties"]["$ref"], "#/$defs/RuleEntry");
    assert_eq!(schema["properties"]["rules"]["$ref"], "#/$defs/Rules");
    assert_eq!(
        schema["$defs"]["Override"]["properties"]["rules"]["$ref"],
        "#/$defs/Rules"
    );
}

#[test]
fn every_builtin_documents_itself() {
    let rules = oxcruise_rules::builtin_rules();
    assert_eq!(rules.len(), 17);
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

fn folders(entries: Value) -> Tester {
    Tester::rule("folder-structure", json!({ "folders": entries }))
}

#[test]
fn folder_structure_require() {
    let t = folders(json!([
        { "path": "src/pages/{page}", "require": ["codeowners.ts", "index.{ts,tsx}"] }
    ]))
    .with_files(&["src/pages/a/codeowners.ts", "src/pages/b/codeowners.ts"]);
    // `index.ts` or `index.tsx` is one of; a page is only a folder directly in `src/pages`
    t.pass(&[
        "src/pages/a/index.ts",
        "src/pages/b/index.tsx",
        "src/pages/b/components/Item.tsx",
    ]);
    insta::assert_snapshot!(
        "folder_structure_require",
        t.fail(&[
            "src/pages/a/index.ts",
            "src/pages/c/index.tsx",
            "src/pages/d/components/Item.tsx",
        ])
    );
}

#[test]
fn folder_structure_allow() {
    let t = folders(json!([
        { "path": "src/modules/{m}", "allow": "!{utils,helpers}", "message": "Put shared helpers in src/lib." },
        { "path": "src/ui/{u}", "allow": ["hooks", "components", "*.{ts,tsx}"] }
    ]));
    t.pass(&[
        "src/modules/a/index.ts",
        "src/modules/a/hooks/use.ts",
        "src/ui/x/hooks/use.ts",
        "src/ui/x/components/Button.tsx",
        "src/ui/x/index.ts",
    ]);
    insta::assert_snapshot!(
        "folder_structure_allow",
        t.fail(&[
            "src/modules/a/utils/x.ts",
            "src/modules/b/helpers/y.ts",
            "src/modules/b/ok.ts",
            "src/ui/x/store/s.ts",
            "src/ui/x/README.md",
            "src/ui/x/components/Button.tsx",
        ])
    );
}

#[test]
fn folder_structure_allow_with_only_negations_allows_everything_else() {
    let t = folders(json!([{ "path": "src", "allow": ["!tmp", "!*.bak"] }]));
    t.pass(&["src/a.ts", "src/lib/b.ts"]);
    let out = t.fail(&["src/a.ts", "src/tmp/b.ts"]);
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.contains("src/tmp"), "{out}");
    let out = t.with_files(&["src/x.bak"]).fail(&["src/a.ts"]);
    assert!(out.contains("src/x.bak"), "{out}");
}

#[test]
fn folder_structure_case() {
    let t = folders(json!([
        { "path": "src/components", "case": "pascal-case" },
        { "path": "src/hooks", "case": "camel-case" },
        { "path": "src/routes", "case": "kebab-case" },
        { "path": "src/jobs", "case": "snake-case" }
    ]))
    .with_files(&["src/components/.gitkeep"]);
    t.pass(&[
        "src/components/Button.tsx",
        "src/components/Button.test.tsx",
        "src/components/UserCard/index.ts",
        "src/hooks/useThing.ts",
        "src/routes/user-list.ts",
        "src/routes/404.ts",
        "src/jobs/nightly_sync.ts",
    ]);
    insta::assert_snapshot!(
        "folder_structure_case",
        t.fail(&[
            "src/components/button.tsx",
            "src/components/User-Card/index.ts",
            "src/hooks/UseThing.ts",
            "src/hooks/use_thing.ts",
            "src/routes/userList.ts",
            "src/routes/user--list.ts",
            "src/jobs/nightly-sync.ts",
            "src/jobs/_sync.ts",
        ])
    );
}

#[test]
fn folder_structure_substitutes_captures() {
    // the file has the name of its folder
    let t = folders(json!([{ "path": "src/components/{c}", "require": "{c}.tsx" }]));
    t.pass(&[
        "src/components/Button/Button.tsx",
        "src/components/Card/Card.tsx",
    ]);
    insta::assert_snapshot!(
        "folder_structure_capture",
        t.fail(&[
            "src/components/Button/Button.tsx",
            "src/components/Card/index.tsx",
        ])
    );
}

#[test]
fn folder_structure_companion_files() {
    // a file match: the options apply to the folder of the file, with its captures
    let t = folders(json!([{
        "path": ["src/components/**/{c}.tsx", "!**/*.{test,stories}.tsx"],
        "require": ["{c}.test.tsx", "{c}.stories.tsx"]
    }]));
    t.pass(&[
        "src/components/Button/Button.tsx",
        "src/components/Button/Button.test.tsx",
        "src/components/Button/Button.stories.tsx",
    ]);
    insta::assert_snapshot!(
        "folder_structure_companions",
        t.fail(&[
            "src/components/Button/Button.tsx",
            "src/components/Button/Button.test.tsx",
            "src/components/Card/Card.tsx",
            "src/components/Card/Card.stories.tsx",
        ])
    );
}

#[test]
fn folder_structure_require_reaches_into_folders() {
    let t =
        folders(json!([{ "path": "packages/{p}", "require": ["package.json", "src/index.ts"] }]))
            .with_files(&["packages/a/package.json", "packages/b/package.json"]);
    t.pass(&["packages/a/src/index.ts", "packages/b/src/index.ts"]);
    let out = t.fail(&["packages/a/src/index.ts", "packages/b/src/other.ts"]);
    assert!(out.contains("packages/b/src/index.ts"), "{out}");
    assert_eq!(out.lines().count(), 1, "{out}");
}

#[test]
fn folder_structure_overlapping_entries_all_apply() {
    let t = folders(json!([
        { "path": "src/{a}", "require": "index.ts" },
        { "path": "src/{a}", "allow": "!tmp" },
        { "path": "src/**", "case": "kebab-case" }
    ]));
    let out = t.fail(&["src/one/Mixed.ts", "src/one/tmp/x.ts"]);
    insta::assert_snapshot!("folder_structure_overlapping", out);
}

#[test]
fn folder_structure_the_root_folder_is_a_dot() {
    let t = folders(json!([{ "path": ".", "require": "README.md" }]));
    let out = t.fail(&["src/a.ts"]);
    assert!(out.contains("`.` must contain `README.md`"), "{out}");
    let t = folders(json!([{ "path": ".", "require": "README.md" }])).with_files(&["README.md"]);
    t.pass(&["src/a.ts"]);
}

#[test]
fn folder_structure_negated_path_excludes_a_folder() {
    let t = folders(json!([
        { "path": ["src/pages/{page}", "!src/pages/legacy"], "require": "owner.ts" }
    ]));
    t.pass(&["src/pages/legacy/index.ts"]);
    t.fail(&["src/pages/new/index.ts"]);
}

#[test]
fn folder_structure_reports_every_bad_option_at_once() {
    let cfg: oxcruise_config::OxcruiseConfig = serde_json::from_value(json!({
        "rules": { "folder-structure": { "folders": [
            { "path": "a" },
            { "path": "b", "require": "!c" },
            { "path": [], "case": "kebab-case" },
            { "path": "d", "case": "loud-case" },
            { "path": "e", "casee": "kebab-case" },
            { "path": "f", "allow": "x", "require": "y" }
        ] } }
    }))
    .unwrap();
    let err = oxcruise_rules::Engine::new(&cfg, None).err().unwrap();
    assert_eq!(err.len(), 1, "{err:?}");
    // the deserialiser stops at the first bad value of the shape; the entry checks run together
    let cfg: oxcruise_config::OxcruiseConfig = serde_json::from_value(json!({
        "rules": { "folder-structure": { "folders": [
            { "path": "a" },
            { "path": "b", "require": "!c" },
            { "path": [], "case": "kebab-case" },
            { "path": "f", "allow": "x", "require": "y" }
        ] } }
    }))
    .unwrap();
    let err = oxcruise_rules::Engine::new(&cfg, None).err().unwrap();
    let text = err.join("\n");
    for needle in ["folders[0]", "folders[1]", "folders[2]"] {
        assert!(text.contains(needle), "{needle}: {text}");
    }
    assert!(!text.contains("folders[3]"), "{text}");
}

#[test]
fn folder_structure_does_not_need_a_tree_when_off() {
    let cfg: oxcruise_config::OxcruiseConfig = serde_json::from_value(json!({})).unwrap();
    let engine = oxcruise_rules::Engine::new(&cfg, None).unwrap();
    assert!(!engine.wants_file_tree());
    let cfg: oxcruise_config::OxcruiseConfig = serde_json::from_value(json!({
        "rules": { "folder-structure": { "folders": [{ "path": "a", "case": "kebab-case" }] } }
    }))
    .unwrap();
    assert!(
        oxcruise_rules::Engine::new(&cfg, None)
            .unwrap()
            .wants_file_tree()
    );
}

#[test]
fn folder_structure_ignore_file_silences_a_module_but_not_a_missing_file() {
    let t = folders(json!([
        { "path": "src/ui/{u}", "allow": "*.ts", "require": "owner.ts" }
    ]));
    // `x.css` is not a module, so no comment can reach it; the module `y.tsx` carries one
    let out = t
        .with_files(&["src/ui/a/x.css"])
        .fail(&["src/ui/a/y.tsx [ignore-file=folder-structure]"]);
    assert!(out.contains("src/ui/a/x.css"), "{out}");
    assert!(out.contains("src/ui/a/owner.ts"), "{out}");
    assert!(!out.contains("y.tsx"), "{out}");
    // the same comment with the rule name of another rule does not help
    let out = folders(json!([{ "path": "src/ui/{u}", "allow": "*.ts" }]))
        .fail(&["src/ui/a/y.tsx [ignore-file=layers]"]);
    assert!(out.contains("y.tsx"), "{out}");
}
