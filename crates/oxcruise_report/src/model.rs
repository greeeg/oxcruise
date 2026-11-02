use std::path::PathBuf;

use oxcruise_core::{
    DepKinds, ExportKind, Graph, ModuleId, ModuleKind, ModuleSystem, Severity, Subject, UsedNames,
    Violation,
};
use oxcruise_graph::analyses::strongly_connected;
use oxcruise_resolve::WorkspacePackage;
use serde::{Deserialize, Serialize};

/// 2: field names are camelCase; violations on exports and files have their own `kind`;
/// `summary.totalModules` counts the entries of `modules`.
pub const RESULT_VERSION: u32 = 2;

/// Whether the export `name` of a module is used.
pub type ExportUsed<'a> = dyn Fn(ModuleId, &str) -> bool + 'a;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CruiseResult {
    pub version: u32,
    /// Where source snippets are read from; not part of the JSON.
    #[serde(skip)]
    pub root: PathBuf,
    pub modules: Vec<ModuleOut>,
    pub violations: Vec<ViolationOut>,
    pub summary: Summary,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleOut {
    pub path: String,
    pub kind: ModuleKind,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub package: Option<String>,
    /// Present in the graph but not explored (`doNotFollow`, depth limit, npm).
    pub not_followed: bool,
    pub orphan: bool,
    pub dependencies: Vec<DepOut>,
    pub dependents: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub parse_error: Option<String>,
    /// What the module exports and whether anything uses it (only when symbols were extracted).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub exports: Vec<ExportOut>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOut {
    pub name: String,
    pub kind: ExportKind,
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub is_type: bool,
    /// Where it is declared; absent when unknown.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub col: Option<u32>,
    /// Path of the module a re-export reads from.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub from: Option<String>,
    /// Name read from `from`: `a` in `export { a as b } from "x"`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub imported: Option<String>,
    /// Some module imports it, or it is part of an entry point's interface.
    pub used: bool,
}

/// The names a dependency uses from its target. In JSON, `"*"` or the list of names.
#[derive(Debug, Clone, PartialEq)]
pub enum NamesOut {
    /// Anything.
    Any,
    /// Exports used; `E.A` is a member of an export.
    Some(Vec<String>),
}

impl Serialize for NamesOut {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            NamesOut::Any => s.serialize_str("*"),
            NamesOut::Some(names) => names.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for NamesOut {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        /// Only the string `"*"`.
        #[derive(Deserialize)]
        enum Star {
            #[serde(rename = "*")]
            Any,
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Star(Star),
            List(Vec<String>),
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Star(Star::Any) => NamesOut::Any,
            Raw::List(names) => NamesOut::Some(names),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepOut {
    pub to: String,
    pub to_kind: ModuleKind,
    pub specifier: String,
    pub kinds: DepKinds,
    pub module_system: ModuleSystem,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub col: Option<u32>,
    /// On an import cycle of the whole graph (type-only imports included), whether or not a rule
    /// reports it.
    pub circular: bool,
    /// The cycle a `no-circular` violation reports through this dependency, starting here.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cycle: Vec<CycleHop>,
    /// The rules that reported this dependency.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub rules: Vec<RuleRef>,
    /// Only when symbols were extracted.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub names: Option<NamesOut>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CycleHop {
    pub module: String,
    /// Kinds of the edge leaving this module towards the next hop.
    pub kinds: DepKinds,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleRef {
    pub name: String,
    pub severity: Severity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViolationKind {
    Dependency,
    Module,
    Cycle,
    Reachability,
    /// An export of a module (`from` is the module).
    Export,
    /// A file that is not a module, such as a `package.json` (`from` is its path).
    File,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViolationOut {
    pub rule: String,
    pub severity: Severity,
    pub kind: ViolationKind,
    pub from: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub col: Option<u32>,
    pub message: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cycle: Vec<CycleHop>,
    /// Length of the specifier as written, for underlining (including quotes).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub specifier_len: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub error: usize,
    pub warn: usize,
    pub suppressed: usize,
    pub total_modules: usize,
    pub total_dependencies: usize,
}

/// How many violations a run found, before `--quiet` or `--fix` took any out of sight.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub error: usize,
    pub warn: usize,
    pub suppressed: usize,
}

impl Totals {
    pub fn of(violations: &[Violation], suppressed: usize) -> Totals {
        let count = |s| violations.iter().filter(|v| v.severity == s).count();
        Totals {
            error: count(Severity::Error),
            warn: count(Severity::Warn),
            suppressed,
        }
    }
}

impl CruiseResult {
    /// Assemble the result of a run. `violations` must already be sorted and suppression-filtered;
    /// they may be fewer than `totals` counts (see [`Totals`]).
    pub fn build(
        graph: &Graph,
        workspace: &[WorkspacePackage],
        root: PathBuf,
        violations: &[Violation],
        totals: Totals,
        warnings: Vec<String>,
        // `Some` when symbols were extracted: whether `(module, export name)` is used
        symbols: Option<&ExportUsed<'_>>,
    ) -> CruiseResult {
        let name = |m| graph.module(m).path.clone();

        // Rules that hit each edge, and the cycles that pass through each.
        let mut edge_rules: Vec<Vec<RuleRef>> = vec![vec![]; graph.edge_count()];
        let mut edge_cycles: Vec<Option<Vec<CycleHop>>> = vec![None; graph.edge_count()];
        let scc = strongly_connected(graph, &|_| true);
        for v in violations {
            let r = RuleRef {
                name: v.rule.clone(),
                severity: v.severity,
            };
            match v.edge {
                Some(_) if !v.path.is_empty() => {
                    // A cycle is reported once, but every edge on it is part of it.
                    let all = hops(graph, &v.path);
                    for (i, step) in v.path.iter().enumerate() {
                        let Some(e) = step.edge else { continue };
                        edge_rules[e.idx()].push(r.clone());
                        edge_cycles[e.idx()].get_or_insert_with(|| {
                            let mut rotated = all.clone();
                            rotated.rotate_left(i);
                            rotated
                        });
                    }
                }
                Some(e) => edge_rules[e.idx()].push(r),
                None => {}
            }
        }

        let modules = graph
            .module_ids()
            .filter(|&m| graph.module(m).kind != ModuleKind::Unresolved)
            .map(|m| {
                let module = graph.module(m);
                let mut dependents: Vec<String> = graph
                    .in_edge_ids(m)
                    .iter()
                    .map(|&e| graph.edge(e).from)
                    // a module that imports itself is not its own dependent
                    .filter(|&from| from != m)
                    .map(name)
                    .collect();
                dependents.dedup();
                let dependencies = graph
                    .out_edge_ids(m)
                    .map(|id| {
                        let e = graph.edge(id);
                        let cycle = edge_cycles[id.idx()].take().unwrap_or_default();
                        DepOut {
                            to: name(e.to),
                            to_kind: graph.module(e.to).kind,
                            specifier: e.specifier.clone(),
                            kinds: e.kinds,
                            module_system: e.module_system,
                            line: e.loc.map(|l| l.line),
                            col: e.loc.map(|l| l.col),
                            circular: scc.is_cyclic_edge(graph, id),
                            cycle,
                            rules: std::mem::take(&mut edge_rules[id.idx()]),
                            names: symbols.map(|_| match &e.used {
                                UsedNames::All => NamesOut::Any,
                                UsedNames::Names(n) => {
                                    NamesOut::Some(n.iter().map(|s| s.to_string()).collect())
                                }
                            }),
                        }
                    })
                    .collect();
                ModuleOut {
                    path: module.path.clone(),
                    kind: module.kind,
                    package: module
                        .package
                        .and_then(|i| workspace.get(i as usize))
                        .map(|p| p.name.clone()),
                    not_followed: module.not_followed,
                    orphan: module.kind == ModuleKind::Local
                        && graph.in_degree(m) == 0
                        && graph.out_degree(m) == 0,
                    dependencies,
                    dependents,
                    parse_error: module.parse_error.clone(),
                    exports: symbols.map_or(vec![], |used| {
                        module
                            .exports
                            .iter()
                            .map(|x| ExportOut {
                                name: x.name.clone(),
                                kind: x.kind,
                                is_type: x.is_type,
                                // line 0 is "unknown"
                                line: (x.loc.line != 0).then_some(x.loc.line),
                                col: (x.loc.line != 0).then_some(x.loc.col),
                                from: x.from.map(name),
                                imported: x.imported.clone(),
                                used: used(m, &x.name),
                            })
                            .collect()
                    }),
                }
            })
            .collect();

        let out = violation_outs(graph, violations);

        CruiseResult {
            version: RESULT_VERSION,
            root,
            summary: Summary::new(graph, totals),
            modules,
            violations: out,
            warnings,
        }
    }
}

/// The violations of a run in their output form.
pub fn violation_outs(graph: &Graph, violations: &[Violation]) -> Vec<ViolationOut> {
    let name = |m| graph.module(m).path.clone();
    violations
        .iter()
        .map(|v| {
            let kind = match (&v.subject, v.edge, v.path.len()) {
                (Subject::Export(_), ..) => ViolationKind::Export,
                (Subject::File(_), ..) => ViolationKind::File,
                (_, Some(_), n) if n > 0 => ViolationKind::Cycle,
                (_, Some(_), _) => ViolationKind::Dependency,
                (_, None, n) if n > 0 => ViolationKind::Reachability,
                (_, None, _) => ViolationKind::Module,
            };
            ViolationOut {
                rule: v.rule.clone(),
                severity: v.severity,
                kind,
                from: match &v.subject {
                    Subject::File(f) => f.clone(),
                    _ => name(v.from),
                },
                to: v.to.map(name),
                line: v.loc.map(|l| l.line),
                col: v.loc.map(|l| l.col),
                message: v.message.clone(),
                cycle: if kind == ViolationKind::Cycle || kind == ViolationKind::Reachability {
                    hops(graph, &v.path)
                } else {
                    vec![]
                },
                specifier_len: v.edge.map(|e| graph.edge(e).specifier.len() as u32 + 2),
            }
        })
        .collect()
}

impl Summary {
    pub fn new(graph: &Graph, totals: Totals) -> Summary {
        Summary {
            error: totals.error,
            warn: totals.warn,
            suppressed: totals.suppressed,
            // what `modules` lists: unresolved specifiers are leaves, not modules
            total_modules: graph
                .module_ids()
                .filter(|&m| graph.module(m).kind != ModuleKind::Unresolved)
                .count(),
            total_dependencies: graph.edge_count(),
        }
    }
}

fn hops(graph: &Graph, path: &[oxcruise_core::CycleStep]) -> Vec<CycleHop> {
    path.iter()
        .map(|s| CycleHop {
            module: graph.module(s.module).path.clone(),
            kinds: s.edge.map_or(DepKinds::empty(), |e| graph.edge(e).kinds),
        })
        .collect()
}

#[cfg(test)]
mod names_tests {
    use super::NamesOut;

    #[test]
    fn names_are_a_star_or_a_list_in_json() {
        assert_eq!(serde_json::to_string(&NamesOut::Any).unwrap(), r#""*""#);
        let some = NamesOut::Some(vec!["a".into(), "E.A".into()]);
        assert_eq!(serde_json::to_string(&some).unwrap(), r#"["a","E.A"]"#);
        for names in [NamesOut::Any, some, NamesOut::Some(vec![])] {
            let json = serde_json::to_string(&names).unwrap();
            assert_eq!(serde_json::from_str::<NamesOut>(&json).unwrap(), names);
        }
        // only the star means anything
        assert!(serde_json::from_str::<NamesOut>(r#""all""#).is_err());
    }
}

#[cfg(test)]
mod build_tests {
    use oxcruise_core::{Edge, Module, ModuleSystem, UsedNames};

    use super::*;

    fn edge(from: u32, to: u32) -> Edge {
        Edge {
            from: ModuleId(from),
            to: ModuleId(to),
            specifier: "./x".into(),
            kinds: DepKinds::IMPORT,
            module_system: ModuleSystem::Es6,
            loc: None,
            suppressed: vec![],
            used: UsedNames::All,
        }
    }

    fn built(violations: &[Violation]) -> CruiseResult {
        // modules are ordered by path: a.ts, b.ts, missing
        let modules = vec![
            Module::new("a.ts", ModuleKind::Local),
            Module::new("b.ts", ModuleKind::Local),
            Module::new("missing", ModuleKind::Unresolved),
        ];
        let graph = Graph::build(modules, vec![edge(0, 0), edge(0, 1), edge(0, 2)]);
        CruiseResult::build(
            &graph,
            &[],
            PathBuf::new(),
            violations,
            Totals::of(violations, 0),
            vec![],
            None,
        )
    }

    #[test]
    fn a_module_is_not_its_own_dependent() {
        let r = built(&[]);
        let a = r.modules.iter().find(|m| m.path == "a.ts").unwrap();
        let b = r.modules.iter().find(|m| m.path == "b.ts").unwrap();
        assert!(a.dependents.is_empty(), "{:?}", a.dependents);
        assert_eq!(b.dependents, ["a.ts"]);
    }

    #[test]
    fn total_modules_counts_what_modules_lists() {
        let r = built(&[]);
        assert_eq!(r.modules.len(), 2);
        assert_eq!(r.summary.total_modules, 2);
        assert_eq!(r.summary.total_dependencies, 3);
    }

    #[test]
    fn exports_and_files_have_their_own_kind() {
        let mut export = Violation::module("r", Severity::Warn, ModuleId(0), "x".into());
        export.subject = Subject::Export(0);
        let mut file = Violation::module("r", Severity::Warn, ModuleId(0), "y".into());
        file.subject = Subject::File("package.json".into());
        let module = Violation::module("r", Severity::Warn, ModuleId(1), "z".into());
        let r = built(&[export, file, module]);
        let kinds: Vec<_> = r
            .violations
            .iter()
            .map(|v| (v.kind, v.from.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                (ViolationKind::Export, "a.ts"),
                (ViolationKind::File, "package.json"),
                (ViolationKind::Module, "b.ts"),
            ]
        );
        let json = serde_json::to_value(&r.violations[1]).unwrap();
        assert_eq!(json["kind"], "file");
    }
}
