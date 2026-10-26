//! dependency-cruiser's `cruise-result` JSON shape, so existing consumers keep working.

use std::io::{self, Write};

use oxcruise_core::{DepKinds, ModuleKind};
use serde_json::{Value, json};

use crate::{CruiseResult, CycleHop, ViolationKind};

fn kinds(k: DepKinds) -> Value {
    Value::Array(k.names().map(|n| json!(n)).collect())
}

/// depcruise lists the modules of a cycle starting at `to`, each with the kinds of the
/// edge that leads into it; our hops are (module, edge leaving it).
fn cycle(hops: &[CycleHop]) -> Value {
    let n = hops.len();
    Value::Array(
        (0..n)
            .map(|i| json!({ "name": hops[(i + 1) % n].module, "dependencyTypes": kinds(hops[i].kinds) }))
            .collect(),
    )
}

fn rule(name: &str, sev: oxcruise_core::Severity) -> Value {
    json!({ "name": name, "severity": sev.as_str() })
}

/// depcruise reports only what a dependency resolves to when it could not be resolved
/// (`unknown`), not how it was written.
fn dep_kinds(d: &crate::DepOut) -> DepKinds {
    // `url`, `worker` and `call` are oxcruise additions that the cruise-result schema does not know.
    let known = d.kinds & !(DepKinds::URL | DepKinds::WORKER | DepKinds::CALL);
    if d.to_kind == ModuleKind::Unresolved {
        known & !DepKinds::SYNTAX_ANY
    } else {
        known
    }
}

/// depcruise strips the `node:` prefix of built-in modules.
fn module_name(d: &crate::DepOut) -> &str {
    if d.to_kind == ModuleKind::Core {
        d.specifier.strip_prefix("node:").unwrap_or(&d.specifier)
    } else {
        &d.specifier
    }
}

pub fn render(result: &CruiseResult, out: &mut dyn Write) -> io::Result<()> {
    let mut modules: Vec<Value> = result
        .modules
        .iter()
        .map(|m| {
            let module_rules: Vec<Value> = result
                .violations
                .iter()
                .filter(|v| v.to.is_none() && v.from == m.path)
                .map(|v| rule(&v.rule, v.severity))
                .collect();
            let deps: Vec<Value> = m
                .dependencies
                .iter()
                .map(|d| {
                    let mut o = json!({
                        "resolved": d.to,
                        "coreModule": d.to_kind == ModuleKind::Core,
                        "followable": d.to_kind == ModuleKind::Source,
                        "couldNotResolve": d.to_kind == ModuleKind::Unresolved,
                        "dynamic": d.kinds.contains(DepKinds::DYNAMIC_IMPORT),
                        "module": module_name(d),
                        "moduleSystem": d.module_system.as_str(),
                        "exoticallyRequired": d.kinds.contains(DepKinds::EXOTIC_REQUIRE),
                        "dependencyTypes": kinds(dep_kinds(d)),
                        "valid": d.rules.iter().all(|r| r.severity == oxcruise_core::Severity::Ignore),
                    });
                    let map = o.as_object_mut().expect("object");
                    if !d.rules.is_empty() {
                        map.insert("rules".into(), Value::Array(d.rules.iter().map(|r| rule(&r.name, r.severity)).collect()));
                    }
                    map.insert("circular".into(), json!(d.circular));
                    if d.circular {
                        map.insert("cycle".into(), cycle(&d.cycle));
                    }
                    o
                })
                .collect();
            let valid = module_rules.is_empty() && m.dependencies.iter().all(|d| d.rules.is_empty());
            let mut o = json!({
                "source": m.path,
                "dependencies": deps,
                "dependents": m.dependents,
                "orphan": m.orphan,
                "valid": valid,
            });
            let map = o.as_object_mut().expect("object");
            match m.kind {
                ModuleKind::Core => {
                    map.insert("coreModule".into(), json!(true));
                    map.insert("followable".into(), json!(false));
                }
                ModuleKind::Stub => {
                    map.insert("followable".into(), json!(false));
                    map.insert("coreModule".into(), json!(false));
                    map.insert("couldNotResolve".into(), json!(false));
                }
                _ => {}
            }
            if m.not_followed && m.kind == ModuleKind::Source {
                map.insert("matchesDoNotFollow".into(), json!(true));
            }
            if !module_rules.is_empty() {
                map.insert("rules".into(), Value::Array(module_rules));
            }
            o
        })
        .collect();

    // Unresolved specifiers are modules too, so consumers can see what depends on them.
    let mut unresolved: std::collections::BTreeMap<&str, Vec<&str>> =
        std::collections::BTreeMap::new();
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
        modules.push(json!({
            "source": spec,
            "dependencies": [],
            "dependents": dependents,
            "orphan": false,
            "valid": true,
            "followable": false,
            "coreModule": false,
            "couldNotResolve": true,
        }));
    }

    let violations: Vec<Value> = result
        .violations
        .iter()
        .map(|v| {
            let ty = match v.kind {
                ViolationKind::Dependency => "dependency",
                ViolationKind::Module => "module",
                ViolationKind::Cycle => "cycle",
                ViolationKind::Reachability => "reachability",
            };
            let mut o = json!({
                "type": ty,
                "from": v.from,
                "to": v.to.clone().unwrap_or_else(|| v.from.clone()),
                "rule": rule(&v.rule, v.severity),
            });
            if v.kind == ViolationKind::Cycle {
                o.as_object_mut()
                    .expect("object")
                    .insert("cycle".into(), cycle(&v.cycle));
            }
            o
        })
        .collect();

    let doc = json!({
        "modules": modules,
        "summary": {
            "violations": violations,
            "error": result.summary.error,
            "warn": result.summary.warn,
            "info": result.summary.info,
            "ignore": result.summary.suppressed,
            "totalCruised": result.summary.total_modules,
            "totalDependenciesCruised": result.summary.total_dependencies,
            "optionsUsed": {},
        },
    });
    serde_json::to_writer_pretty(&mut *out, &doc).map_err(io::Error::other)?;
    writeln!(out)
}
