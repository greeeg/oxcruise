use std::path::PathBuf;

use oxcruise_config::OxcruiseConfig;
use oxcruise_graph::{CruiseOptions, cruise};
use oxcruise_report::{CruiseResult, Totals};
use oxcruise_rules::Engine;
use oxcruise_visualize::{VizData, data_script};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/monorepo")
        .canonicalize()
        .unwrap()
}

fn result() -> CruiseResult {
    let root = fixture();
    let out = cruise(&CruiseOptions::new(&root, vec!["packages".into()])).unwrap();
    let config = OxcruiseConfig::default();
    let run = Engine::new(&config, None).unwrap().run(
        &out.graph,
        &root,
        &out.workspace,
        &out.manifests,
        false,
    );
    CruiseResult::build(
        &out.graph,
        &out.workspace,
        root,
        &run.violations,
        Totals::of(&run.violations, run.suppressed),
        run.warnings,
        None,
    )
}

/// Sizes depend on the checkout (line endings), so the tests use the path length.
fn data() -> VizData {
    VizData::build(&result(), &|p| p.len() as u32, 0)
}

#[test]
fn the_monorepo_payload() {
    let mut data = data();
    data.root = "<root>".into();
    insta::assert_json_snapshot!(data);
}

#[test]
fn externals_live_under_one_folder() {
    let data = data();
    let external = data.dirs.name.iter().position(|n| n == "(external)");
    if let Some(external) = external {
        let external = external as u32;
        for (i, kind) in data.modules.kind.iter().enumerate() {
            if *kind != 0 {
                let mut dir = data.modules.dir[i];
                while data.dirs.parent[dir as usize] != 0 {
                    dir = data.dirs.parent[dir as usize];
                }
                assert_eq!(dir, external, "{}", data.modules.name[i]);
                assert_eq!(data.modules.size[i], 0);
            }
        }
    }
    // columns agree
    let m = &data.modules;
    let n = m.dir.len();
    assert!(
        [
            m.name.len(),
            m.kind.len(),
            m.size.len(),
            m.flags.len(),
            m.package.len()
        ]
        .iter()
        .all(|&l| l == n)
    );
    assert_eq!(data.edges.from.len(), data.edges.to.len());
}

#[test]
fn data_js_is_a_parseable_string() {
    let mut data = data();
    data.root = "it's a \\ </script> \u{2028} root".into();
    let js = data_script(&data);
    let prefix = "globalThis.OXCRUISE_DATA = JSON.parse('";
    assert!(js.starts_with(prefix), "{js}");
    assert!(!js.contains("</script"));
    assert!(!js.contains('\u{2028}'));
    // undo the JS string escapes and read it back
    let body = js[prefix.len()..].trim_end().strip_suffix("');").unwrap();
    let unescaped = body
        .replace("\\u2028", "\u{2028}")
        .replace("<\\/script", "</script")
        .replace("\\'", "'")
        .replace("\\\\", "\\");
    let back: serde_json::Value = serde_json::from_str(&unescaped).unwrap();
    assert_eq!(back["root"], data.root.as_str());
    assert_eq!(back["version"], 2);
}

#[test]
fn write_site_keeps_other_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("keep.txt"), "mine").unwrap();
    oxcruise_visualize::write_site(&dir.path().join("out"), &result()).unwrap();
    oxcruise_visualize::write_site(dir.path(), &result()).unwrap();
    for f in ["index.html", "app.js", "app.css", "data.js"] {
        assert!(dir.path().join(f).exists(), "{f}");
        assert!(dir.path().join("out").join(f).exists(), "out/{f}");
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("keep.txt")).unwrap(),
        "mine"
    );
}
