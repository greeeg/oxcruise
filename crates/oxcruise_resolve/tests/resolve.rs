use std::path::PathBuf;

use oxcruise_core::DepKinds;
use oxcruise_resolve::{ResolveConfig, Resolver, Target};

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
    let res = r.resolve(&app("index.ts"), "./util");
    assert_eq!(res.kinds, DepKinds::LOCAL);
    assert_eq!(local_rel(&r, &res.target), "packages/app/src/util.ts");
}

#[test]
fn tsconfig_paths_use_the_nearest_tsconfig() {
    let r = resolver();
    let res = r.resolve(&app("index.ts"), "@app/helpers");
    assert_eq!(local_rel(&r, &res.target), "packages/app/src/helpers.ts");
    assert!(res.kinds.contains(
        DepKinds::ALIASED_TSCONFIG_PATHS | DepKinds::ALIASED_TSCONFIG | DepKinds::ALIASED
    ));

    // lib has its own tsconfig: its alias works there ...
    let lib_index = fixture().join("packages/lib/src/index.ts");
    let ok = r.resolve(&lib_index, "@lib/inner");
    assert_eq!(local_rel(&r, &ok.target), "packages/lib/src/inner.ts");
    // ... and app's alias does not leak into it (#565).
    let bad = r.resolve(&lib_index, "@app/helpers");
    assert!(matches!(bad.target, Target::Unresolved(_)), "{bad:?}");
    assert_eq!(bad.kinds, DepKinds::UNKNOWN);
}

#[test]
fn subpath_imports() {
    let r = resolver();
    let res = r.resolve(&app("index.ts"), "#internal/secret");
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
        let res = r.resolve(&f, spec);
        assert!(matches!(res.target, Target::Npm { .. }), "{spec}: {res:?}");
        assert_eq!(res.kinds & DepKinds::NPM_ANY, kind, "{spec}");
    };
    check("lodash", DepKinds::NPM);
    check("vitest", DepKinds::NPM_DEV);
    check("react", DepKinds::NPM_PEER);
    check("fsevents", DepKinds::NPM_OPTIONAL);
    check("left-pad", DepKinds::NPM_NO_PKG);
    assert!(
        r.resolve(&f, "left-pad")
            .kinds
            .contains(DepKinds::DEPRECATED)
    );
    assert!(!r.resolve(&f, "lodash").kinds.contains(DepKinds::DEPRECATED));
}

#[test]
fn builtins() {
    let r = resolver();
    let f = app("index.ts");
    for spec in ["fs", "node:fs"] {
        let res = r.resolve(&f, spec);
        assert_eq!(res.target, Target::Core("fs".into()));
        assert_eq!(res.kinds, DepKinds::CORE);
    }
    let sys = r.resolve(&f, "sys");
    assert_eq!(sys.kinds, DepKinds::CORE | DepKinds::DEPRECATED);
}

#[test]
fn unresolvable_and_skipped() {
    let r = resolver();
    let f = app("index.ts");
    let res = r.resolve(&f, "./missing");
    assert!(matches!(res.target, Target::Unresolved(_)));
    assert_eq!(res.kinds, DepKinds::UNKNOWN);
    assert_eq!(
        r.resolve(&f, "https://example.com/x.js").target,
        Target::Skip
    );
}

#[test]
fn workspace_packages_resolve_to_source() {
    let r = resolver();
    let res = r.resolve(&app("index.ts"), "@acme/lib");
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
    let res = r.resolve(&app("index.ts"), "~/util");
    assert_eq!(local_rel(&r, &res.target), "packages/app/src/util.ts");
    assert!(res.kinds.contains(DepKinds::ALIASED_WEBPACK));
}
