use std::collections::BTreeMap;
use std::path::PathBuf;

use oxcruise_core::{DepKinds, Graph, ModuleKind, ModuleSystem, Severity, Violation};
use oxcruise_resolve::WorkspacePackage;
use serde::{Deserialize, Serialize};

pub const RESULT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    pub circular: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cycle: Vec<CycleHop>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub rules: Vec<RuleRef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
pub struct Summary {
    pub error: usize,
    pub warn: usize,
    pub suppressed: usize,
    pub total_modules: usize,
    pub total_dependencies: usize,
}

impl CruiseResult {
    /// Assemble the result of a run. `violations` must already be sorted and suppression-filtered.
    pub fn build(
        graph: &Graph,
        workspace: &[WorkspacePackage],
        root: PathBuf,
        violations: &[Violation],
        suppressed: usize,
        warnings: Vec<String>,
    ) -> CruiseResult {
        let name = |m| graph.module(m).path.clone();

        // Rules that hit each edge, and the cycles that pass through each.
        let mut edge_rules: BTreeMap<u32, Vec<RuleRef>> = BTreeMap::new();
        let mut edge_cycles: BTreeMap<u32, Vec<CycleHop>> = BTreeMap::new();
        let mut module_rules: BTreeMap<u32, Vec<RuleRef>> = BTreeMap::new();
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
                        edge_rules.entry(e.0).or_default().push(r.clone());
                        let mut rotated = all.clone();
                        rotated.rotate_left(i);
                        edge_cycles.entry(e.0).or_insert(rotated);
                    }
                }
                Some(e) => edge_rules.entry(e.0).or_default().push(r),
                None => module_rules.entry(v.from.0).or_default().push(r),
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
                    .map(|&e| name(graph.edge(e).from))
                    .collect();
                dependents.dedup();
                let dependencies = graph
                    .out_edge_ids(m)
                    .map(|id| {
                        let e = graph.edge(id);
                        let cycle = edge_cycles.get(&id.0).cloned().unwrap_or_default();
                        DepOut {
                            to: name(e.to),
                            to_kind: graph.module(e.to).kind,
                            specifier: e.specifier.clone(),
                            kinds: e.kinds,
                            module_system: e.module_system,
                            line: e.loc.map(|l| l.line),
                            col: e.loc.map(|l| l.col),
                            circular: !cycle.is_empty(),
                            cycle,
                            rules: edge_rules.get(&id.0).cloned().unwrap_or_default(),
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
                    orphan: module.kind == ModuleKind::Source
                        && graph.in_degree(m) == 0
                        && graph.out_degree(m) == 0,
                    dependencies,
                    dependents,
                    parse_error: module.parse_error.clone(),
                }
            })
            .collect();

        let out: Vec<ViolationOut> = violations
            .iter()
            .map(|v| {
                let kind = match (v.edge, v.path.len()) {
                    (Some(_), n) if n > 0 => ViolationKind::Cycle,
                    (Some(_), _) => ViolationKind::Dependency,
                    (None, n) if n > 0 => ViolationKind::Reachability,
                    (None, _) => ViolationKind::Module,
                };
                ViolationOut {
                    rule: v.rule.clone(),
                    severity: v.severity,
                    kind,
                    from: name(v.from),
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
            .collect();

        let count = |s| violations.iter().filter(|v| v.severity == s).count();
        CruiseResult {
            version: RESULT_VERSION,
            root,
            summary: Summary {
                error: count(Severity::Error),
                warn: count(Severity::Warn),
                suppressed,
                total_modules: graph.module_count(),
                total_dependencies: graph.edge_count(),
            },
            modules,
            violations: out,
            warnings,
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
