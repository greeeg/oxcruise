use std::path::PathBuf;

use oxcruise_config::OxcruiseConfig;
use oxcruise_graph::{CruiseOptions, cruise};
use oxcruise_report::{CruiseResult, ExternalReporters, OutputFormat, ReporterOptions, render};
use oxcruise_rules::{Engine, builtin_rules};

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
    let run = Engine::new(&config, None)
        .unwrap()
        .run(&out.graph, &root, &out.workspace);
    CruiseResult::build(
        &out.graph,
        &out.workspace,
        root,
        &run.violations,
        run.suppressed,
        run.warnings,
    )
}

fn rendered(format: &str, opts: &ReporterOptions) -> String {
    let mut buf = vec![];
    render(&format.parse().unwrap(), &result(), opts, None, &mut buf).unwrap();
    String::from_utf8(buf).unwrap()
}

fn plain() -> ReporterOptions {
    ReporterOptions::default()
}

#[test]
fn default_format() {
    insta::assert_snapshot!(rendered("default", &plain()));
}

#[test]
fn long_adds_rule_docs() {
    let docs = builtin_rules()
        .into_iter()
        .map(|r| (r.name.to_string(), r.docs.to_string()))
        .collect();
    let out = rendered(
        "long",
        &ReporterOptions {
            rule_docs: docs,
            ..Default::default()
        },
    );
    assert!(out.contains("About `no-circular`"), "{out}");
    assert!(
        out.contains("Reports dependencies that are part of a circular chain"),
        "{out}"
    );
    assert!(!rendered("default", &plain()).contains("About `"));
}

#[test]
fn color_only_when_asked() {
    assert!(!rendered("default", &plain()).contains('\x1b'));
    assert!(
        rendered(
            "default",
            &ReporterOptions {
                color: true,
                ..Default::default()
            }
        )
        .contains("\x1b[1;31m")
    );
}

#[test]
fn json() {
    insta::assert_snapshot!(rendered("json", &plain()));
}

#[test]
fn github() {
    insta::assert_snapshot!(rendered("github", &plain()));
}

#[test]
fn graph_formats() {
    insta::assert_snapshot!("dot", rendered("dot", &plain()));
    insta::assert_snapshot!("mermaid", rendered("mermaid", &plain()));
    insta::assert_snapshot!("text", rendered("text", &plain()));
}

#[test]
fn saved_json_renders_the_same_as_a_live_run() {
    let live = result();
    let saved: CruiseResult = serde_json::from_str(&serde_json::to_string(&live).unwrap()).unwrap();
    for f in ["dot", "mermaid", "text", "github"] {
        let fmt: OutputFormat = f.parse().unwrap();
        let (mut a, mut b) = (vec![], vec![]);
        render(&fmt, &live, &plain(), None, &mut a).unwrap();
        render(&fmt, &saved, &plain(), None, &mut b).unwrap();
        assert_eq!(
            String::from_utf8(a).unwrap(),
            String::from_utf8(b).unwrap(),
            "{f}"
        );
    }
}

struct Upper;
impl ExternalReporters for Upper {
    fn render(&self, name: &str, result_json: &str) -> Result<String, String> {
        let v: serde_json::Value = serde_json::from_str(result_json).map_err(|e| e.to_string())?;
        Ok(format!(
            "{name}: {} violations",
            v["violations"].as_array().unwrap().len()
        ))
    }
}

#[test]
fn external_reporters() {
    let mut buf = vec![];
    render(
        &"acme/count".parse().unwrap(),
        &result(),
        &plain(),
        Some(&Upper),
        &mut buf,
    )
    .unwrap();
    assert!(String::from_utf8(buf).unwrap().starts_with("acme/count: "));

    let err = render(
        &"acme/count".parse().unwrap(),
        &result(),
        &plain(),
        None,
        &mut vec![],
    )
    .unwrap_err();
    assert!(err.to_string().contains("npm package"), "{err}");
}
