//! dependency-cruiser's `cruise-result` JSON shape, so existing consumers keep working.
//!
//! Built from borrowed, typed structs and streamed to the writer: the result of a big repo with
//! many cycles is hundreds of MB, and a `serde_json::Value` tree would cost several times that.

use std::collections::BTreeMap;
use std::io::{self, Write};

use oxcruise_core::{DepKinds, ModuleKind, Severity};
use serde::{Serialize, Serializer};

use crate::{CruiseResult, CycleHop, DepOut, ViolationKind};

/// A set of dependency types, written as an array of names.
#[derive(Clone, Copy)]
struct Types(DepKinds);

impl Serialize for Types {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.0.names())
    }
}

/// depcruise reports only what a dependency resolves to when it could not be resolved
/// (`unknown`), not how it was written, and knows nothing of `url`, `worker` and `call`.
fn dep_types(d: &DepOut) -> Types {
    let known = d.kinds & !(DepKinds::URL | DepKinds::WORKER | DepKinds::CALL);
    Types(if d.to_kind == ModuleKind::Unresolved {
        known & !DepKinds::SYNTAX_ANY
    } else {
        known
    })
}

/// depcruise strips the `node:` prefix of built-in modules.
fn module_name(d: &DepOut) -> &str {
    if d.to_kind == ModuleKind::Core {
        d.specifier.strip_prefix("node:").unwrap_or(&d.specifier)
    } else {
        &d.specifier
    }
}

#[derive(Serialize)]
struct Rule<'a> {
    name: &'a str,
    severity: &'static str,
}

fn rule(name: &str, severity: Severity) -> Rule<'_> {
    Rule {
        name,
        severity: severity.as_str(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CycleStep<'a> {
    name: &'a str,
    dependency_types: Types,
}

/// depcruise lists the modules of a cycle starting at `to`, each with the kinds of the edge
/// that leads into it; our hops are (module, edge leaving it).
fn cycle(hops: &[CycleHop]) -> Vec<CycleStep<'_>> {
    let n = hops.len();
    (0..n)
        .map(|i| CycleStep {
            name: &hops[(i + 1) % n].module,
            dependency_types: Types(hops[i].kinds),
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Dependency<'a> {
    resolved: &'a str,
    core_module: bool,
    followable: bool,
    could_not_resolve: bool,
    dynamic: bool,
    module: &'a str,
    module_system: &'static str,
    exotically_required: bool,
    dependency_types: Types,
    valid: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    rules: Vec<Rule<'a>>,
    circular: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cycle: Vec<CycleStep<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Module<'a> {
    source: &'a str,
    dependencies: Vec<Dependency<'a>>,
    dependents: Vec<&'a str>,
    orphan: bool,
    valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    followable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    core_module: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    could_not_resolve: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    matches_do_not_follow: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    rules: Vec<Rule<'a>>,
}

#[derive(Serialize)]
struct Violation<'a> {
    r#type: &'static str,
    from: &'a str,
    to: &'a str,
    rule: Rule<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cycle: Option<Vec<CycleStep<'a>>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary<'a> {
    violations: Vec<Violation<'a>>,
    error: usize,
    warn: usize,
    info: usize,
    ignore: usize,
    total_cruised: usize,
    total_dependencies_cruised: usize,
    options_used: BTreeMap<(), ()>,
}

#[derive(Serialize)]
struct Document<'a> {
    modules: Vec<Module<'a>>,
    summary: Summary<'a>,
}

pub fn render(result: &CruiseResult, out: &mut dyn Write) -> io::Result<()> {
    let mut modules: Vec<Module<'_>> = result
        .modules
        .iter()
        .map(|m| {
            let module_rules: Vec<Rule<'_>> = result
                .violations
                .iter()
                .filter(|v| v.to.is_none() && v.from == m.path)
                .map(|v| rule(&v.rule, v.severity))
                .collect();
            let dependencies: Vec<Dependency<'_>> = m
                .dependencies
                .iter()
                .map(|d| Dependency {
                    resolved: &d.to,
                    core_module: d.to_kind == ModuleKind::Core,
                    followable: d.to_kind == ModuleKind::Source,
                    could_not_resolve: d.to_kind == ModuleKind::Unresolved,
                    dynamic: d.kinds.contains(DepKinds::DYNAMIC_IMPORT),
                    module: module_name(d),
                    module_system: d.module_system.as_str(),
                    exotically_required: d.kinds.contains(DepKinds::EXOTIC_REQUIRE),
                    dependency_types: dep_types(d),
                    valid: d.rules.iter().all(|r| r.severity == Severity::Ignore),
                    rules: d.rules.iter().map(|r| rule(&r.name, r.severity)).collect(),
                    circular: d.circular,
                    cycle: if d.circular { cycle(&d.cycle) } else { vec![] },
                })
                .collect();
            let valid =
                module_rules.is_empty() && m.dependencies.iter().all(|d| d.rules.is_empty());
            let (followable, core_module, could_not_resolve) = match m.kind {
                ModuleKind::Core => (Some(false), Some(true), None),
                ModuleKind::Stub => (Some(false), Some(false), Some(false)),
                _ => (None, None, None),
            };
            Module {
                source: &m.path,
                dependencies,
                dependents: m.dependents.iter().map(String::as_str).collect(),
                orphan: m.orphan,
                valid,
                followable,
                core_module,
                could_not_resolve,
                matches_do_not_follow: (m.not_followed && m.kind == ModuleKind::Source)
                    .then_some(true),
                rules: module_rules,
            }
        })
        .collect();

    // Unresolved specifiers are modules too, so consumers can see what depends on them.
    let mut unresolved: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for m in &result.modules {
        for d in m
            .dependencies
            .iter()
            .filter(|d| d.to_kind == ModuleKind::Unresolved)
        {
            let v = unresolved.entry(d.to.as_str()).or_default();
            if !v.contains(&m.path.as_str()) {
                v.push(&m.path);
            }
        }
    }
    for (spec, dependents) in unresolved {
        modules.push(Module {
            source: spec,
            dependencies: vec![],
            dependents,
            orphan: false,
            valid: true,
            followable: Some(false),
            core_module: Some(false),
            could_not_resolve: Some(true),
            matches_do_not_follow: None,
            rules: vec![],
        });
    }

    let violations = result
        .violations
        .iter()
        .map(|v| Violation {
            r#type: match v.kind {
                ViolationKind::Dependency => "dependency",
                ViolationKind::Module => "module",
                ViolationKind::Cycle => "cycle",
                ViolationKind::Reachability => "reachability",
            },
            from: &v.from,
            to: v.to.as_deref().unwrap_or(&v.from),
            rule: rule(&v.rule, v.severity),
            cycle: (v.kind == ViolationKind::Cycle).then(|| cycle(&v.cycle)),
        })
        .collect();

    let doc = Document {
        modules,
        summary: Summary {
            violations,
            error: result.summary.error,
            warn: result.summary.warn,
            info: result.summary.info,
            ignore: result.summary.suppressed,
            total_cruised: result.summary.total_modules,
            total_dependencies_cruised: result.summary.total_dependencies,
            options_used: BTreeMap::new(),
        },
    };
    // not pretty-printed: for a big graph the whitespace alone is tens of MB
    serde_json::to_writer(&mut *out, &doc).map_err(io::Error::other)?;
    writeln!(out)
}
