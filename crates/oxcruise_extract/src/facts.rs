pub use oxcruise_core::ExportKind;
use oxcruise_core::{DepKinds, ExportMember, LineIndex, Loc, ModuleSystem, Span};
use rustc_hash::FxHashMap;
use serde::Serialize;

/// What to look for beyond the syntax every file has. JSDoc imports, triple-slash directives
/// and `new URL("./x", import.meta.url)` are always extracted.
#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    /// Callee names whose first string argument is a dependency, e.g. `jest.mock`.
    pub calls: Vec<String>,
    /// Also collect what is exported and which names of each import are used (for the
    /// `unused-*` rules). Costs an extra walk over identifier references, so it is opt-in.
    pub symbols: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RawDependency {
    pub specifier: String,
    /// Syntactic kinds only (`import`, `require`, `type-only`, ...). Resolution
    /// adds `local`, `npm-*`, `aliased-*`, ...
    pub kinds: DepKinds,
    pub module_system: ModuleSystem,
    pub span: Span,
    pub loc: Loc,
    /// Rules silenced for this dependency by a comment; `"*"` means all.
    pub suppressed: Vec<String>,
}

impl RawDependency {
    pub fn is_type_only(&self) -> bool {
        self.kinds.contains(DepKinds::TYPE_ONLY)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Suppression {
    /// Rules named by the comment; empty means every rule.
    pub rules: Vec<String>,
    pub loc: Loc,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportFact {
    pub name: String,
    pub kind: ExportKind,
    pub from: Option<String>,
    pub is_type: bool,
    /// Name read from `from`: `a` in `export { a as b } from "x"`. Set with `symbols`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported: Option<String>,
    /// Local binding of a `Local` export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loc: Option<Loc>,
    /// Rules silenced for this export by a comment; `"*"` means all.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suppressed: Vec<String>,
    /// Members of an exported enum or TypeScript namespace.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<ExportMember>,
    /// The local binding is referenced elsewhere in the file.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub used_in_file: bool,
    /// Earlier export of the same binding under another name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<String>,
    /// Byte range to delete to remove this export from the file; set when that is safe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<(u32, u32)>,
}

impl ExportFact {
    pub(crate) fn new(name: String, kind: ExportKind, from: Option<String>, is_type: bool) -> Self {
        Self {
            name,
            kind,
            from,
            is_type,
            imported: None,
            local: None,
            loc: None,
            suppressed: vec![],
            members: vec![],
            used_in_file: false,
            duplicate_of: None,
            fix: None,
        }
    }
}

/// How the file uses an imported binding.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub enum UsedRefs {
    /// Used in a way that cannot be narrowed (or not analysed).
    #[default]
    Opaque,
    /// Only ever read as `binding.member`; empty when the binding is never referenced.
    Members(Vec<String>),
}

impl UsedRefs {
    pub(crate) fn is_opaque(&self) -> bool {
        matches!(self, UsedRefs::Opaque)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportFact {
    pub specifier: String,
    /// `default`, `*` or the exported name.
    pub imported: String,
    pub local: String,
    pub is_type: bool,
    #[serde(skip_serializing_if = "UsedRefs::is_opaque")]
    pub refs: UsedRefs,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FileFacts {
    pub deps: Vec<RawDependency>,
    /// `oxcruise-ignore-file` comments.
    pub file_suppressions: Vec<Suppression>,
    pub exports: Vec<ExportFact>,
    pub imports: Vec<ImportFact>,
    pub has_module_syntax: bool,
    pub parse_error: Option<String>,
}

/// A dependency before location lookup and deduplication.
pub(crate) struct PendingDep {
    pub specifier: String,
    pub kinds: DepKinds,
    pub module_system: ModuleSystem,
    pub span: Span,
    /// Start of the enclosing statement, used to attach `ignore-next-line`.
    pub stmt_start: u32,
}

pub(crate) struct PendingSuppression {
    pub kind: SuppressionKind,
    pub rules: Vec<String>,
    /// Byte offset where the comment starts and ends.
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SuppressionKind {
    NextLine,
    Line,
    File,
}

pub(crate) struct FactsBuilder<'s> {
    pub line_index: &'s LineIndex,
    pub opts: &'s ExtractOptions,
    pub deps: Vec<PendingDep>,
    pub suppressions: Vec<PendingSuppression>,
    pub exports: Vec<ExportFact>,
    /// `(name start, statement start)` of each export, in step with `exports`.
    pub export_pos: Vec<(u32, u32)>,
    pub imports: Vec<ImportFact>,
    pub has_module_syntax: bool,
    pub parse_error: Option<String>,
}

impl<'s> FactsBuilder<'s> {
    pub fn new(line_index: &'s LineIndex, opts: &'s ExtractOptions) -> Self {
        Self {
            line_index,
            opts,
            deps: vec![],
            suppressions: vec![],
            exports: vec![],
            export_pos: vec![],
            imports: vec![],
            has_module_syntax: false,
            parse_error: None,
        }
    }

    pub fn push_dep(
        &mut self,
        specifier: &str,
        kinds: DepKinds,
        module_system: ModuleSystem,
        span: Span,
        stmt_start: u32,
    ) {
        if specifier.is_empty() {
            return;
        }
        self.deps.push(PendingDep {
            specifier: specifier.to_string(),
            kinds,
            module_system,
            span,
            stmt_start,
        });
    }

    pub fn finish(mut self) -> FileFacts {
        let li = self.line_index;
        let mut file_suppressions = vec![];
        for s in self
            .suppressions
            .iter()
            .filter(|s| s.kind == SuppressionKind::File)
        {
            file_suppressions.push(Suppression {
                rules: s.rules.clone(),
                loc: li.loc(s.start),
            });
        }

        // Dedupe on (specifier, module system, type-only): kinds merge and the
        // first occurrence wins the location. A rule stays suppressed only if
        // every occurrence suppresses it, so one unsuppressed `import` of the
        // same module is never hidden by a suppressed sibling.
        let mut index: FxHashMap<(String, ModuleSystem, bool), usize> = FxHashMap::default();
        let mut deps: Vec<RawDependency> = vec![];
        let mut occurrences: Vec<Vec<Vec<String>>> = vec![];
        let mut pending = std::mem::take(&mut self.deps);
        pending.sort_by_key(|d| d.span.start);
        for d in pending {
            let suppressed = self.suppressions_at(d.span.start, d.stmt_start);
            let key = (
                d.specifier.clone(),
                d.module_system,
                d.kinds.contains(DepKinds::TYPE_ONLY),
            );
            match index.get(&key) {
                Some(&i) => {
                    deps[i].kinds |= d.kinds;
                    occurrences[i].push(suppressed);
                }
                None => {
                    index.insert(key, deps.len());
                    occurrences.push(vec![suppressed]);
                    deps.push(RawDependency {
                        loc: li.loc(d.span.start),
                        specifier: d.specifier,
                        kinds: d.kinds,
                        module_system: d.module_system,
                        span: d.span,
                        suppressed: vec![],
                    });
                }
            }
        }
        for (dep, occ) in deps.iter_mut().zip(occurrences) {
            dep.suppressed = intersect_suppressions(&occ);
        }

        let mut exports = std::mem::take(&mut self.exports);
        if self.opts.symbols {
            let mut first_of: FxHashMap<String, String> = FxHashMap::default();
            for (e, &(at, stmt)) in exports.iter_mut().zip(&self.export_pos) {
                e.loc = Some(li.loc(at));
                e.suppressed = self.suppressions_at(at, stmt);
                if let Some(local) = e.local.clone().filter(|l| !l.starts_with('*')) {
                    match first_of.get(&local) {
                        Some(first) => e.duplicate_of = Some(first.clone()),
                        None => {
                            first_of.insert(local, e.name.clone());
                        }
                    }
                }
            }
        }

        FileFacts {
            deps,
            file_suppressions,
            exports,
            imports: self.imports,
            has_module_syntax: self.has_module_syntax,
            parse_error: self.parse_error,
        }
    }

    /// Rules silenced for the thing starting at `at` inside the statement starting at `stmt_start`.
    fn suppressions_at(&self, at: u32, stmt_start: u32) -> Vec<String> {
        let li = self.line_index;
        let dep_line = li.loc(at).line;
        let stmt_line = li.loc(stmt_start).line;
        let mut out = vec![];
        for s in &self.suppressions {
            let end_line = li.loc(s.end.saturating_sub(1).max(s.start)).line;
            let hit = match s.kind {
                SuppressionKind::NextLine => end_line + 1 == stmt_line || end_line + 1 == dep_line,
                SuppressionKind::Line => end_line == dep_line || end_line == stmt_line,
                SuppressionKind::File => false,
            };
            if hit {
                if s.rules.is_empty() {
                    out.push("*".to_string());
                } else {
                    out.extend(s.rules.iter().cloned());
                }
            }
        }
        out
    }
}

/// Rules suppressed in every occurrence; `"*"` in an occurrence matches any rule.
fn intersect_suppressions(occurrences: &[Vec<String>]) -> Vec<String> {
    let covers = |set: &Vec<String>, rule: &str| set.iter().any(|r| r == "*" || r == rule);
    if occurrences.iter().all(|o| o.iter().any(|r| r == "*")) {
        return vec!["*".to_string()];
    }
    let mut out: Vec<String> = vec![];
    for rule in occurrences.iter().flatten().filter(|r| *r != "*") {
        if !out.contains(rule) && occurrences.iter().all(|o| covers(o, rule)) {
            out.push(rule.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::intersect_suppressions;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn intersection() {
        assert_eq!(intersect_suppressions(&[v(&["a", "b"])]), v(&["a", "b"]));
        assert_eq!(
            intersect_suppressions(&[v(&["a", "b"]), v(&["b"])]),
            v(&["b"])
        );
        assert_eq!(intersect_suppressions(&[v(&["a"]), v(&[])]), v(&[]));
        assert_eq!(intersect_suppressions(&[v(&["*"]), v(&["a"])]), v(&["a"]));
        assert_eq!(intersect_suppressions(&[v(&["*"]), v(&["*"])]), v(&["*"]));
    }
}
