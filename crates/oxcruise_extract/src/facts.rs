use oxcruise_core::{DepKinds, LineIndex, Loc, ModuleSystem, Span};
use rustc_hash::FxHashMap;
use serde::Serialize;

/// What to look for beyond the syntax every file has.
#[derive(Debug, Clone, Default)]
pub struct ExtractOptions {
    /// Callee names treated like `require` (depcruise `exoticRequireStrings`).
    pub exotic_require_strings: Vec<String>,
    /// Callee names whose first string argument is a dependency, e.g. `jest.mock`.
    pub calls: Vec<String>,
    /// Look for `@import` tags and `import("x")` types inside JSDoc comments.
    pub jsdoc: bool,
    /// Skip triple-slash directives.
    pub skip_triple_slash: bool,
    /// Do not treat `new URL("./x", import.meta.url)` and `new Worker(new URL(..))` as dependencies.
    pub skip_urls: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportKind {
    Local,
    Reexport,
    Star,
    Namespace,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExportFact {
    pub name: String,
    pub kind: ExportKind,
    pub from: Option<String>,
    pub is_type: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportFact {
    pub specifier: String,
    /// `default`, `*` or the exported name.
    pub imported: String,
    pub local: String,
    pub is_type: bool,
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
            let suppressed = self.suppressions_for(&d);
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

        FileFacts {
            deps,
            file_suppressions,
            exports: self.exports,
            imports: self.imports,
            has_module_syntax: self.has_module_syntax,
            parse_error: self.parse_error,
        }
    }

    fn suppressions_for(&self, d: &PendingDep) -> Vec<String> {
        let li = self.line_index;
        let dep_line = li.loc(d.span.start).line;
        let stmt_line = li.loc(d.stmt_start).line;
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
