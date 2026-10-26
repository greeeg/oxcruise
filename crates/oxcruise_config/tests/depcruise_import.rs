use std::path::Path;

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
    { "name": "uses-license", "severity": "error", "from": {}, "to": { "license": "GPL" } },
    { "name": "folder-rule", "scope": "folder", "severity": "warn", "from": {}, "to": { "path": "x" } }
  ],
  "allowed": [{ "from": {}, "to": {} }],
  "allowedSeverity": "error",
  "required": [
    { "name": "pages-use-layout", "severity": "error", "module": { "path": "^src/pages/" }, "to": { "path": "^src/layout/" } }
  ],
  "options": {
    "doNotFollow": { "path": "node_modules", "dependencyTypes": ["npm"] },
    "exclude": { "path": ["^dist", "[.]stories[.]tsx$"] },
    "includeOnly": "^src",
    "maxDepth": 0,
    "tsConfig": { "fileName": "tsconfig.json" },
    "enhancedResolveOptions": { "conditionNames": ["import", "types"], "exportsFields": ["exports"], "extensions": [".ts"] },
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

    assert_eq!(
        cfg.forbid
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>(),
        [
            "no-circular",
            "no-orphans",
            "not-to-dev-dep",
            "only-via-index"
        ]
    );
    assert_eq!(cfg.allow[0].name, "not-in-allowed");
    assert_eq!(cfg.extends, ["oxcruise:strict"]);
    assert_eq!(cfg.follow.max_depth, None);
}

#[test]
fn loads_dependency_cruiser_js_through_the_loader() {
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
         module.exports = {\n  forbidden: [{ name: 'no-circular', severity: 'error', from: {}, to: { circular: true } }],\n  options: { doNotFollow: { path: 'node_modules' } },\n};\n",
    )
    .unwrap();
    let loaded = ConfigLoader::new(&NodeSubprocessLoader as &dyn JsConfigLoader)
        .load(&p)
        .unwrap();
    assert!(loaded.imported_from_depcruise);
    assert_eq!(loaded.config.forbid.len(), 1);
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    assert!(depcruise::looks_like_depcruise(
        Path::new("x.json"),
        &serde_json::json!({"forbidden": []})
    ));
    assert!(!depcruise::looks_like_depcruise(
        Path::new("x.json"),
        &serde_json::json!({"rules": {}})
    ));
}
