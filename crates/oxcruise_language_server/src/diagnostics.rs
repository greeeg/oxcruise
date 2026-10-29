//! `ViolationOut` -> LSP `Diagnostic`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;

use lsp_types::{
    CodeDescription, Diagnostic, DiagnosticRelatedInformation, DiagnosticSeverity, Location,
    NumberOrString, Position, Range, Uri,
};
use oxcruise_core::Severity;
use oxcruise_report::{CycleHop, ViolationKind, ViolationOut};

use crate::source::{Encoding, Source};
use crate::uri::path_to_uri;

pub const SOURCE: &str = "oxcruise";
const RULE_DOCS: &str = "https://github.com/greeeg/oxcruise/blob/master/docs/rules.md";

/// Link to the docs of a built-in rule; plugin and custom rules have none.
pub fn rule_docs_url(rule: &str) -> Option<String> {
    static BUILTIN: OnceLock<Vec<&'static str>> = OnceLock::new();
    let builtin = BUILTIN.get_or_init(|| {
        oxcruise_rules::builtin_rules()
            .into_iter()
            .map(|r| r.name)
            .collect()
    });
    builtin
        .contains(&rule)
        .then(|| format!("{RULE_DOCS}#{rule}"))
}

/// Files read once per conversion.
#[derive(Default)]
pub struct Files {
    sources: HashMap<PathBuf, Source>,
}

impl Files {
    pub fn get(&mut self, path: &Path) -> &Source {
        self.sources
            .entry(path.to_path_buf())
            .or_insert_with(|| Source::read(path))
    }
}

/// Diagnostics of a run by absolute file path. `base` is the directory the module paths are
/// relative to.
pub fn by_file(
    base: &Path,
    violations: &[ViolationOut],
    enc: Encoding,
) -> BTreeMap<PathBuf, Vec<Diagnostic>> {
    let mut files = Files::default();
    let mut out: BTreeMap<PathBuf, Vec<Diagnostic>> = BTreeMap::new();
    for v in violations {
        let path = base.join(&v.from);
        let diagnostic = diagnostic(base, &path, v, enc, &mut files);
        out.entry(path).or_default().push(diagnostic);
    }
    out
}

pub fn diagnostic(
    base: &Path,
    path: &Path,
    v: &ViolationOut,
    enc: Encoding,
    files: &mut Files,
) -> Diagnostic {
    Diagnostic {
        range: range(files.get(path), v, enc),
        severity: Some(match v.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warn => DiagnosticSeverity::WARNING,
        }),
        code: Some(NumberOrString::String(v.rule.clone())),
        code_description: rule_docs_url(&v.rule)
            .and_then(|href| Uri::from_str(&href).ok())
            .map(|href| CodeDescription { href }),
        source: Some(SOURCE.to_string()),
        message: v.message.clone(),
        related_information: related(base, v, enc, files),
        tags: None,
        data: serde_json::to_value(v.kind).ok(),
    }
}

/// The specifier for a violation on an import; otherwise the first line of the file.
fn range(source: &Source, v: &ViolationOut, enc: Encoding) -> Range {
    if let (Some(line), Some(col)) = (v.line, v.col) {
        let (l, start) = source.position(line, col, enc);
        let end = match v.specifier_len {
            Some(len) => source.position(line, col + len, enc).1,
            None => source.line_end(l, enc),
        };
        return Range::new(Position::new(l, start), Position::new(l, end));
    }
    Range::new(
        Position::new(0, 0),
        Position::new(0, source.line_end(0, enc)),
    )
}

/// One entry per hop of a cycle or path, at the import that leaves the hop's module.
fn related(
    base: &Path,
    v: &ViolationOut,
    enc: Encoding,
    files: &mut Files,
) -> Option<Vec<DiagnosticRelatedInformation>> {
    if v.cycle.is_empty() {
        return None;
    }
    let is_cycle = v.kind == ViolationKind::Cycle;
    let info = v
        .cycle
        .iter()
        .enumerate()
        .filter_map(|(i, hop)| {
            let next = v.cycle.get(i + 1).or(is_cycle.then(|| &v.cycle[0]));
            let path = base.join(&hop.module);
            let (line, col) = hop_position(files.get(&path), hop, enc);
            let at = Position::new(line, col);
            Some(DiagnosticRelatedInformation {
                location: Location::new(path_to_uri(&path)?, Range::new(at, at)),
                message: match next {
                    Some(n) => format!("{} imports {}", hop.module, n.module),
                    None => format!("{} ends the path", hop.module),
                },
            })
        })
        .collect();
    Some(info)
}

fn hop_position(source: &Source, hop: &CycleHop, enc: Encoding) -> (u32, u32) {
    match (hop.line, hop.col) {
        (Some(line), Some(col)) => source.position(line, col, enc),
        _ => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxcruise_core::{DepKinds, Severity};

    fn violation(rule: &str, kind: ViolationKind) -> ViolationOut {
        ViolationOut {
            rule: rule.into(),
            severity: Severity::Error,
            kind,
            from: "a.ts".into(),
            to: Some("b.ts".into()),
            line: Some(2),
            col: Some(10),
            message: "msg".into(),
            cycle: vec![],
            specifier_len: Some(6),
        }
    }

    fn tmp(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(name), text).unwrap();
        }
        dir
    }

    #[test]
    fn edge_violation_underlines_the_specifier() {
        let dir = tmp(&[("a.ts", "// x\nimport './b';\n")]);
        let mut v = violation("no-circular", ViolationKind::Dependency);
        (v.col, v.specifier_len) = (Some(8), Some(5));
        let map = by_file(dir.path(), &[v], Encoding::Utf16);
        let d = &map[&dir.path().join("a.ts")][0];
        assert_eq!(
            d.range,
            Range::new(Position::new(1, 7), Position::new(1, 12))
        );
        assert_eq!(d.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(d.source.as_deref(), Some("oxcruise"));
        assert_eq!(d.code, Some(NumberOrString::String("no-circular".into())));
        assert!(
            d.code_description
                .as_ref()
                .unwrap()
                .href
                .as_str()
                .ends_with("docs/rules.md#no-circular")
        );
    }

    #[test]
    fn utf16_columns_follow_non_ascii_text() {
        // `é` and `😀` before the specifier: 6 bytes, 3 units
        let dir = tmp(&[("a.ts", "é😀import './b';\n")]);
        let mut v = violation("x", ViolationKind::Dependency);
        v.line = Some(1);
        v.col = Some(14);
        let at = |enc| {
            let map = by_file(dir.path(), &[v.clone()], enc);
            map[&dir.path().join("a.ts")][0].range
        };
        assert_eq!(at(Encoding::Utf8).start, Position::new(0, 13));
        assert_eq!(at(Encoding::Utf16).start, Position::new(0, 10));
        assert_eq!(at(Encoding::Utf16).end, Position::new(0, 16));
    }

    #[test]
    fn module_violation_covers_the_first_line() {
        let dir = tmp(&[("a.ts", "export const é = 1;\nmore\n")]);
        let mut v = violation("no-orphans", ViolationKind::Module);
        (v.line, v.col, v.specifier_len, v.to) = (None, None, None, None);
        v.severity = Severity::Warn;
        let map = by_file(dir.path(), &[v], Encoding::Utf16);
        let d = &map[&dir.path().join("a.ts")][0];
        assert_eq!(
            d.range,
            Range::new(Position::new(0, 0), Position::new(0, 19))
        );
        assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
        assert_eq!(d.data, Some(serde_json::json!("module")));
        assert!(d.related_information.is_none());
    }

    #[test]
    fn missing_file_falls_back_to_the_graph_columns() {
        let dir = tmp(&[]);
        let v = violation("x", ViolationKind::Dependency);
        let map = by_file(dir.path(), &[v], Encoding::Utf16);
        let d = &map[&dir.path().join("a.ts")][0];
        assert_eq!(
            d.range,
            Range::new(Position::new(1, 9), Position::new(1, 15))
        );
    }

    #[test]
    fn cycles_list_every_hop() {
        let dir = tmp(&[("a.ts", "import './b';\n"), ("b.ts", "\n\nimport './a';\n")]);
        let hop = |module: &str, line, col| CycleHop {
            module: module.into(),
            kinds: DepKinds::empty(),
            line: Some(line),
            col: Some(col),
        };
        let mut v = violation("no-circular", ViolationKind::Cycle);
        (v.line, v.col) = (Some(1), Some(8));
        v.cycle = vec![hop("a.ts", 1, 8), hop("b.ts", 3, 8)];
        let map = by_file(dir.path(), &[v], Encoding::Utf8);
        let related = map[&dir.path().join("a.ts")][0]
            .related_information
            .clone()
            .unwrap();
        let summary: Vec<_> = related
            .iter()
            .map(|r| (r.message.as_str(), r.location.range.start))
            .collect();
        assert_eq!(
            summary,
            [
                ("a.ts imports b.ts", Position::new(0, 7)),
                ("b.ts imports a.ts", Position::new(2, 7)),
            ]
        );
        assert!(related[1].location.uri.as_str().ends_with("/b.ts"));
    }

    #[test]
    fn only_builtin_rules_have_docs() {
        assert!(rule_docs_url("no-orphans").is_some());
        assert!(rule_docs_url("my-plugin/rule").is_none());
    }
}
