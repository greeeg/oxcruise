use std::path::PathBuf;

use oxcruise_core::DepKinds;
use oxcruise_resolve::{ResolveConfig, ResolveMode, Resolver, Target};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/monorepo")
        .canonicalize()
        .unwrap()
}

fn resolver() -> Resolver {
    Resolver::new(ResolveConfig::new(fixture()))
}

fn app(file: &str) -> PathBuf {
    fixture().join("packages/app/src").join(file)
}

fn local_rel(r: &Resolver, t: &Target) -> String {
    match t {
        Target::Local(p) => r.relative(p),
        other => panic!("expected local, got {other:?}"),
    }
}

#[test]
fn relative_import_is_local() {
    let r = resolver();
    let res = r.resolve(&app("index.ts"), "./util", ResolveMode::Esm);
    assert_eq!(res.kinds, DepKinds::LOCAL);
    assert_eq!(local_rel(&r, &res.target), "packages/app/src/util.ts");
}

#[test]
fn tsconfig_paths_use_the_nearest_tsconfig() {
    let r = resolver();
    let res = r.resolve(&app("index.ts"), "@app/helpers", ResolveMode::Esm);
    assert_eq!(local_rel(&r, &res.target), "packages/app/src/helpers.ts");
    assert!(res.kinds.contains(
        DepKinds::ALIASED_TSCONFIG_PATHS | DepKinds::ALIASED_TSCONFIG | DepKinds::ALIASED
    ));

    // lib has its own tsconfig: its alias works there ...
    let lib_index = fixture().join("packages/lib/src/index.ts");
    let ok = r.resolve(&lib_index, "@lib/inner", ResolveMode::Esm);
    assert_eq!(local_rel(&r, &ok.target), "packages/lib/src/inner.ts");
    // ... and app's alias does not leak into it (#565).
    let bad = r.resolve(&lib_index, "@app/helpers", ResolveMode::Esm);
    assert!(matches!(bad.target, Target::Unresolved(_)), "{bad:?}");
    assert_eq!(bad.kinds, DepKinds::UNKNOWN);
}

#[test]
fn subpath_imports() {
    let r = resolver();
    let res = r.resolve(&app("index.ts"), "#internal/secret", ResolveMode::Esm);
    assert_eq!(
        local_rel(&r, &res.target),
        "packages/app/src/internal/secret.ts"
    );
    assert!(res.kinds.contains(DepKinds::ALIASED_SUBPATH_IMPORT));
}

#[test]
fn npm_sections() {
    let r = resolver();
    let f = app("index.ts");
    let check = |spec: &str, kind: DepKinds| {
        let res = r.resolve(&f, spec, ResolveMode::Esm);
        assert!(matches!(res.target, Target::Npm { .. }), "{spec}: {res:?}");
        assert_eq!(res.kinds & DepKinds::NPM_ANY, kind, "{spec}");
    };
    check("lodash", DepKinds::NPM);
    check("vitest", DepKinds::NPM_DEV);
    check("react", DepKinds::NPM_PEER);
    check("fsevents", DepKinds::NPM_OPTIONAL);
    check("left-pad", DepKinds::NPM_NO_PKG);
    assert!(
        r.resolve(&f, "left-pad", ResolveMode::Esm)
            .kinds
            .contains(DepKinds::DEPRECATED)
    );
    assert!(
        !r.resolve(&f, "lodash", ResolveMode::Esm)
            .kinds
            .contains(DepKinds::DEPRECATED)
    );
}

#[test]
fn builtins() {
    let r = resolver();
    let f = app("index.ts");
    for spec in ["fs", "node:fs"] {
        let res = r.resolve(&f, spec, ResolveMode::Esm);
        assert_eq!(res.target, Target::Core("fs".into()));
        assert_eq!(res.kinds, DepKinds::CORE);
    }
    let sys = r.resolve(&f, "sys", ResolveMode::Esm);
    assert_eq!(sys.kinds, DepKinds::CORE | DepKinds::DEPRECATED);
}

#[test]
fn unresolvable_and_skipped() {
    let r = resolver();
    let f = app("index.ts");
    let res = r.resolve(&f, "./missing", ResolveMode::Esm);
    assert!(matches!(res.target, Target::Unresolved(_)));
    assert_eq!(res.kinds, DepKinds::UNKNOWN);
    assert_eq!(
        r.resolve(&f, "https://example.com/x.js", ResolveMode::Esm)
            .target,
        Target::Skip
    );
}

#[test]
fn workspace_packages_resolve_to_source() {
    let r = resolver();
    let res = r.resolve(&app("index.ts"), "@acme/lib", ResolveMode::Esm);
    assert_eq!(local_rel(&r, &res.target), "packages/lib/src/index.ts");
    assert!(
        res.kinds
            .contains(DepKinds::LOCAL | DepKinds::ALIASED_WORKSPACE | DepKinds::ALIASED)
    );

    let ws = r.workspace();
    assert_eq!(
        ws.packages
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["@acme/app", "@acme/lib"]
    );
    let idx = ws.package_of(&app("index.ts")).unwrap();
    assert_eq!(ws.packages[idx].name, "@acme/app");
    assert_eq!(ws.package_of(&fixture().join("package.json")), None);
}

#[test]
fn config_alias() {
    let mut cfg = ResolveConfig::new(fixture());
    cfg.alias = vec![(
        "~".into(),
        vec![
            fixture()
                .join("packages/app/src")
                .to_string_lossy()
                .into_owned(),
        ],
    )];
    let r = Resolver::new(cfg);
    let res = r.resolve(&app("index.ts"), "~/util", ResolveMode::Esm);
    assert_eq!(local_rel(&r, &res.target), "packages/app/src/util.ts");
    assert!(res.kinds.contains(DepKinds::ALIASED_WEBPACK));
}

#[test]
fn dual_packages_follow_the_import_syntax() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let write = |p: &str, c: &str| {
        let path = root.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, c).unwrap();
    };
    write(
        "package.json",
        r#"{ "name": "app", "dependencies": { "dual": "1.0.0" } }"#,
    );
    write("src/a.ts", "");
    write(
        "node_modules/dual/package.json",
        // `types` first, as TypeScript-era packages usually do
        r#"{ "name": "dual", "exports": { ".": { "types": "./index.d.ts", "import": "./esm.mjs", "require": "./cjs.js" } } }"#,
    );
    write("node_modules/dual/esm.mjs", "");
    write("node_modules/dual/cjs.js", "");
    write("node_modules/dual/index.d.ts", "");

    let r = Resolver::new(ResolveConfig::new(&root));
    let from = root.join("src/a.ts");
    let file = |m| match r.resolve(&from, "dual", m).target {
        Target::Npm { path, .. } => path.file_name().unwrap().to_string_lossy().into_owned(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        file(ResolveMode::Esm),
        "esm.mjs",
        "import() and import pick the `import` condition"
    );
    assert_eq!(
        file(ResolveMode::Cjs),
        "cjs.js",
        "require() picks the `require` condition"
    );
}

#[test]
fn types_packages_are_a_fallback_not_an_addition() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let write = |p: &str, c: &str| {
        let path = root.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, c).unwrap();
    };
    write(
        "package.json",
        r#"{ "name": "app",
             "dependencies": { "prompts": "2" },
             "devDependencies": { "@types/prompts": "2", "@types/typed-only": "1" } }"#,
    );
    write("src/a.ts", "");
    for pkg in ["prompts", "typed-only", "undeclared"] {
        write(
            &format!("node_modules/{pkg}/package.json"),
            &format!(r#"{{ "name": "{pkg}", "main": "index.js" }}"#),
        );
        write(&format!("node_modules/{pkg}/index.js"), "");
    }

    let r = Resolver::new(ResolveConfig::new(&root));
    let kinds = |spec: &str| {
        r.resolve(&root.join("src/a.ts"), spec, ResolveMode::Esm)
            .kinds
            & DepKinds::NPM_ANY
    };
    assert_eq!(
        kinds("prompts"),
        DepKinds::NPM,
        "a dependency whose @types is a devDependency is not a duplicate"
    );
    assert_eq!(
        kinds("typed-only"),
        DepKinds::NPM_DEV,
        "@types declaration counts when the package itself is undeclared"
    );
    assert_eq!(kinds("undeclared"), DepKinds::NPM_NO_PKG);
}

#[test]
fn runtime_files_win_over_declaration_files() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let write = |p: &str, c: &str| {
        let path = root.join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, c).unwrap();
    };
    write(
        "package.json",
        r#"{ "name": "app", "dependencies": { "ignore": "7" } }"#,
    );
    write("src/a.ts", "");
    // no `main`, only `types`: Node falls back to index.js
    write(
        "node_modules/ignore/package.json",
        r#"{ "name": "ignore", "types": "index.d.ts" }"#,
    );
    write("node_modules/ignore/index.js", "");
    write("node_modules/ignore/index.d.ts", "");
    // a declaration-only module is still found
    write("src/types.d.ts", "");

    let r = Resolver::new(ResolveConfig::new(&root));
    let from = root.join("src/a.ts");
    match r.resolve(&from, "ignore", ResolveMode::Esm).target {
        Target::Npm { path, .. } => assert_eq!(path.file_name().unwrap(), "index.js"),
        other => panic!("{other:?}"),
    }
    match r.resolve(&from, "./types", ResolveMode::Esm).target {
        Target::Local(path) => assert_eq!(path.file_name().unwrap(), "types.d.ts"),
        other => panic!("{other:?}"),
    }
}
