use oxcruise_config::{ConfigLoader, JsConfigLoader, NodeSubprocessLoader, depcruise};

const SAMPLE: &str = r#"{
  "forbidden": [
    {
      "name": "no-circular",
      "severity": "warn",
      "comment": "cycles are bad",
      "from": {},
      "to": { "circular": true }
    },
    {
      "name": "no-orphans",
      "severity": "info",
      "from": { "orphan": true, "pathNot": ["(^|/)[.][^/]+[.](js|cjs|mjs|ts|json)$", "[.]d[.]ts$"] },
      "to": {}
    },
    {
      "name": "not-to-dev-dep",
      "severity": "error",
      "from": { "path": "^src", "pathNot": "[.]test[.]ts$" },
      "to": { "dependencyTypes": ["npm-dev"], "dependencyTypesNot": ["type-only"] }
    },
    {
      "name": "only-via-index",
      "severity": "error",
      "from": { "path": "^src/([^/]+)/" },
      "to": { "path": "^src/$1/", "circular": true, "viaOnly": { "pathNot": "^src/shared" } }
    },
    {
      "name": "feature-isolation",
      "severity": "error",
      "comment": "features must not import each other",
      "from": { "path": "^src/features/([^/]+)/" },
      "to": { "path": "^src/features/", "pathNot": "^src/features/$1/" }
    },
    {
      "name": "no-spec-in-src",
      "severity": "error",
      "from": { "path": "^src", "pathNot": "[.]spec[.]ts$" },
      "to": { "path": "[.]spec[.]ts$" }
    },
    {
      "name": "no-lazy-node-in-web",
      "severity": "error",
      "from": { "path": "(^|/)web/" },
      "to": { "path": "^(fs|path)$", "dynamic": true }
    },
    { "name": "no-unresolvable", "severity": "error", "from": {}, "to": { "couldNotResolve": true } },
    {
      "name": "ui-never-reaches-db",
      "severity": "error",
      "from": { "path": "^src/ui/" },
      "to": { "path": "^src/db/", "reachable": true }
    },
    { "name": "uses-license", "severity": "error", "from": {}, "to": { "license": "GPL" } },
    { "name": "lookahead", "severity": "error", "from": { "path": "^(?!src)" }, "to": {} },
    { "name": "capture-not-defined", "severity": "error", "from": { "path": "^src/" }, "to": { "path": "^lib/$1/" } },
    { "name": "folder-rule", "scope": "folder", "severity": "warn", "from": {}, "to": { "path": "x" } }
  ],
  "allowed": [{ "from": {}, "to": {} }],
  "allowedSeverity": "error",
  "required": [
    { "name": "pages-use-layout", "severity": "error", "module": { "path": "^src/pages/" }, "to": { "path": "^src/layout/" } }
  ],
  "options": {
    "doNotFollow": { "path": "node_modules|[.]generated[.]ts$", "dependencyTypes": ["npm"] },
    "exclude": { "path": ["^dist", "[.]stories[.]tsx$"] },
    "includeOnly": "^src",
    "maxDepth": 3,
    "tsConfig": { "fileName": "tsconfig.app.json" },
    "enhancedResolveOptions": {
      "conditionNames": ["import", "require", "node", "default", "source"],
      "exportsFields": ["exports"],
      "extensions": [".ts"],
      "mainFields": ["main"]
    },
    "exoticRequireStrings": ["need"],
    "detectJSDocImports": true,
    "reporterOptions": { "dot": {} },
    "webpackConfig": { "fileName": "webpack.config.js" },
    "progress": { "type": "performance-log" }
  },
  "extends": ["dependency-cruiser/configs/recommended-strict"]
}"#;

#[test]
fn maps_a_realistic_config() {
    let v: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
    let (cfg, warnings) = depcruise::import(&v);

    insta::assert_json_snapshot!("config", cfg);
    insta::assert_json_snapshot!("warnings", warnings);
}

#[test]
fn only_convertible_rules_survive() {
    let v: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
    let (cfg, _) = depcruise::import(&v);
    assert_eq!(
        cfg.forbid
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        [
            "feature-isolation",
            "no-spec-in-src",
            "no-lazy-node-in-web",
            "no-unresolvable",
            "ui-never-reaches-db",
        ]
    );
    // `recommended-strict` is the default categories with suspicious turned up
    assert_eq!(
        cfg.categories[&oxcruise_config::Category::Suspicious],
        oxcruise_config::Level::Error
    );
    assert!(cfg.extends.is_empty());
}

#[test]
fn the_recommended_presets_map_to_categories() {
    let import = |e: &str| {
        depcruise::import(&serde_json::json!({ "extends": [e] }))
            .0
            .categories
    };
    assert!(import("dependency-cruiser/configs/recommended").is_empty());
    assert_eq!(
        import("dependency-cruiser/configs/recommended-warn-only")
            [&oxcruise_config::Category::Correctness],
        oxcruise_config::Level::Warn
    );
}

#[test]
fn every_dropped_rule_is_named_in_a_warning() {
    let v: serde_json::Value = serde_json::from_str(SAMPLE).unwrap();
    let (_, warnings) = depcruise::import(&v);
    for dropped in [
        "no-circular",
        "no-orphans",
        "not-to-dev-dep",
        "only-via-index",
        "uses-license",
        "lookahead",
        "capture-not-defined",
        "folder-rule",
    ] {
        assert!(
            warnings
                .iter()
                .any(|w| w.contains(&format!("rule `{dropped}` was dropped"))),
            "no warning for {dropped}: {warnings:#?}"
        );
    }
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("`allowed`") && w.contains("layers"))
    );
    assert!(warnings.iter().any(|w| w.contains("`required`")));
}

#[test]
fn loads_dependency_cruiser_js_through_the_importer() {
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_err()
    {
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join(".dependency-cruiser.cjs");
    std::fs::write(
        &p,
        "/** @type {import('dependency-cruiser').IConfiguration} */\n\
         module.exports = {\n  forbidden: [{ name: 'no-src-to-lib', severity: 'error', from: { path: '^src' }, to: { path: '^lib' } }],\n  options: { doNotFollow: { path: 'node_modules' } },\n};\n",
    )
    .unwrap();
    let loaded = ConfigLoader::new(&NodeSubprocessLoader as &dyn JsConfigLoader)
        .import_depcruise(&p)
        .unwrap();
    assert_eq!(loaded.config.forbid.len(), 1);
    assert_eq!(loaded.config.forbid[0].name, "no-src-to-lib");
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
}

#[test]
fn relative_extends_are_imported_and_layered() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(
        d.path().join("base.json"),
        r#"{ "forbidden": [{ "name": "base-rule", "severity": "error", "from": { "path": "^a" }, "to": { "path": "^b" } }] }"#,
    )
    .unwrap();
    std::fs::write(
        d.path().join(".dependency-cruiser.json"),
        r#"{ "extends": "./base.json", "forbidden": [{ "name": "own", "severity": "warn", "from": { "path": "^c" }, "to": { "path": "^d" } }] }"#,
    )
    .unwrap();
    let loaded = ConfigLoader::new(&NodeSubprocessLoader as &dyn JsConfigLoader)
        .import_depcruise(&d.path().join(".dependency-cruiser.json"))
        .unwrap();
    let names: Vec<_> = loaded
        .config
        .forbid
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(names, ["base-rule", "own"]);
}
