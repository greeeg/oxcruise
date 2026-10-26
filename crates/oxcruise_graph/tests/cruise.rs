use std::path::PathBuf;

use oxcruise_core::{DepKinds, Graph, ModuleKind};
use oxcruise_graph::{CruiseOptions, PathMatcher, PathPattern, cruise};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/monorepo")
        .canonicalize()
        .unwrap()
}

fn opts() -> CruiseOptions {
    CruiseOptions::new(fixture(), vec!["packages".into()])
}

fn edge_to(g: &Graph, from: &str, spec: &str) -> oxcruise_core::Edge {
    let f = g.find(from).unwrap_or_else(|| panic!("no module {from}"));
    g.out_edges(f)
        .iter()
        .find(|e| e.specifier == spec)
        .unwrap_or_else(|| panic!("no edge {spec}"))
        .clone()
}

#[test]
fn discovers_the_fixture_monorepo() {
    let out = cruise(&opts()).unwrap();
    let g = &out.graph;
    let index = "packages/app/src/index.ts";

    // every source module is found, and only through the entry walk or imports
    for m in [
        index,
        "packages/app/src/util.ts",
        "packages/app/src/helpers.ts",
        "packages/app/src/orphan.ts",
        "packages/app/src/internal/secret.ts",
        "packages/lib/src/index.ts",
        "packages/lib/src/inner.ts",
        "packages/app/src/cjs-thing.js",
    ] {
        assert!(g.find(m).is_some(), "missing {m}");
    }

    let kinds = |spec: &str| edge_to(g, index, spec).kinds;
    assert!(kinds("./util").contains(DepKinds::LOCAL | DepKinds::IMPORT));
    assert!(kinds("@app/helpers").contains(DepKinds::ALIASED_TSCONFIG_PATHS));
    assert!(kinds("vitest").contains(DepKinds::NPM_DEV));
    assert!(kinds("react").contains(DepKinds::NPM_PEER));
    assert!(kinds("node:fs").contains(DepKinds::CORE));
    assert!(kinds("./cjs-thing").contains(DepKinds::REQUIRE));
    assert!(kinds("./lazy").contains(DepKinds::DYNAMIC_IMPORT));
    assert!(kinds("./missing").contains(DepKinds::UNKNOWN));
    assert!(kinds("@acme/lib").contains(DepKinds::ALIASED_WORKSPACE));

    // stubs
    let lodash = g
        .modules()
        .iter()
        .find(|m| m.path.contains("node_modules/lodash"))
        .unwrap();
    assert_eq!(lodash.kind, ModuleKind::Stub);
    let missing = g.find("./missing").unwrap();
    assert_eq!(g.module(missing).kind, ModuleKind::Unresolved);
    assert_eq!(g.module(g.find("fs").unwrap()).kind, ModuleKind::Core);

    // type-only edge keeps its flag, with a line and column
    let helpers = "packages/app/src/helpers.ts";
    let e = edge_to(g, helpers, "./util");
    assert!(e.is_type_only());
    assert_eq!((e.loc.unwrap().line, e.loc.unwrap().col), (1, 27));

    // workspace packages are attached
    let pkg = g.module(g.find(index).unwrap()).package.unwrap();
    assert_eq!(out.workspace[pkg as usize].name, "@acme/app");

    // file-level and edge-level suppression carry through
    let sup = edge_to(g, "packages/app/src/suppressed.ts", "./nope");
    assert_eq!(sup.suppressed, vec!["no-unresolvable"]);
    assert!(
        edge_to(g, "packages/app/src/suppressed.ts", "./also-nope")
            .suppressed
            .is_empty()
    );
}

#[test]
fn exclude_include_and_do_not_follow() {
    let mut o = opts();
    o.exclude = PathMatcher::new(&[PathPattern::Glob("packages/app/src/internal".into())]).unwrap();
    let g = cruise(&o).unwrap().graph;
    assert!(g.find("packages/app/src/internal/secret.ts").is_none());

    let mut o = opts();
    o.include_only = PathMatcher::new(&[PathPattern::Glob("packages/app/**".into())]).unwrap();
    let g = cruise(&o).unwrap().graph;
    assert!(
        g.modules()
            .iter()
            .all(|m| m.path.starts_with("packages/app/")),
        "{:?}",
        g.modules().iter().map(|m| &m.path).collect::<Vec<_>>()
    );

    // do-not-follow keeps the module but not its dependencies
    let mut o = CruiseOptions::new(fixture(), vec!["packages/app/src/index.ts".into()]);
    o.do_not_follow = PathMatcher::new(&[PathPattern::Glob("packages/lib/**".into())]).unwrap();
    let g = cruise(&o).unwrap().graph;
    let lib = g.find("packages/lib/src/index.ts").unwrap();
    assert!(g.module(lib).not_followed);
    assert_eq!(g.out_degree(lib), 0);
    assert!(g.find("packages/lib/src/inner.ts").is_none());
}

#[test]
fn max_depth_uses_the_shortest_path() {
    // index -> cycle-a (depth 1) -> cycle-b (depth 2)
    let mut o = CruiseOptions::new(fixture(), vec!["packages/app/src/index.ts".into()]);
    o.max_depth = Some(1);
    let g = cruise(&o).unwrap().graph;
    let a = g.find("packages/app/src/cycle-a.ts").unwrap();
    assert_eq!(g.out_degree(a), 0, "depth-1 modules are leaves");
    assert!(
        g.find("packages/app/src/cycle-b.ts").is_none(),
        "depth 2 must be cut"
    );

    o.max_depth = Some(2);
    let g = cruise(&o).unwrap().graph;
    assert!(g.find("packages/app/src/cycle-b.ts").is_some());
}

#[test]
fn deterministic_across_thread_counts() {
    let run = |threads| {
        let mut o = opts();
        o.threads = Some(threads);
        cruise(&o).unwrap().graph
    };
    let one = run(1);
    for n in [2, 8] {
        for _ in 0..5 {
            let many = run(n);
            assert_eq!(
                one.modules(),
                many.modules(),
                "modules differ with {n} threads"
            );
            assert_eq!(one.edges(), many.edges(), "edges differ with {n} threads");
        }
    }
}

#[test]
fn glob_entries_and_missing_entry() {
    let o = CruiseOptions::new(fixture(), vec!["packages/*/src/index.ts".into()]);
    let g = cruise(&o).unwrap().graph;
    assert!(g.find("packages/lib/src/index.ts").is_some());
    assert!(cruise(&CruiseOptions::new(fixture(), vec!["nope".into()])).is_err());
}
