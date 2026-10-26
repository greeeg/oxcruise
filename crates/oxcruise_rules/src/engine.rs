use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use oxcruise_config::{Level, OxcruiseConfig, RuleEntry};
use oxcruise_core::{Graph, ModuleId, Severity, Violation};
use oxcruise_graph::PathMatcher;
use oxcruise_graph::PathPattern;
use oxcruise_graph::analyses::BitSet;
use oxcruise_resolve::WorkspacePackage;
use rayon::prelude::*;

use crate::declarative::{AllowSet, CompiledRule, NOT_IN_ALLOWED};
use crate::external::{ExternalRuleId, ExternalRuleSpec, ExternalRules, ExternalTarget};
use crate::meta::{Analyses, AnalysisCache, RuleCtx, Shared};
use crate::registry::RuleEnum;

#[derive(Clone)]
struct Entry {
    level: Level,
    options: serde_json::Value,
}

enum Source {
    Builtin,
    Declarative(Arc<CompiledRule>),
    AllowSet(Arc<AllowSet>),
    External(ExternalRuleId),
}

struct Plan {
    name: String,
    source: Source,
    base: Option<Entry>,
    /// `(override index, entry)`; the override's `files` decide which modules it affects.
    overrides: Vec<(usize, Entry)>,
}

pub struct Engine {
    plans: Vec<Plan>,
    override_files: Vec<PathMatcher>,
    external: Option<Arc<dyn ExternalRules>>,
    report_dead_rules: bool,
}

#[derive(Debug, Default)]
pub struct RunResult {
    /// Sorted: severity, then location.
    pub violations: Vec<Violation>,
    /// Violations silenced by `oxcruise-ignore-*` comments.
    pub suppressed: usize,
    pub warnings: Vec<String>,
}

impl RunResult {
    pub fn count(&self, s: Severity) -> usize {
        self.violations.iter().filter(|v| v.severity == s).count()
    }
}

fn rank(s: Severity) -> u8 {
    match s {
        Severity::Error => 0,
        Severity::Warn => 1,
        Severity::Info => 2,
        Severity::Ignore => 3,
    }
}

fn entry_of(name: &str, e: &RuleEntry) -> Result<Entry, String> {
    let (level, options) = e.parts().map_err(|m| format!("rule `{name}`: {m}"))?;
    Ok(Entry { level, options })
}

impl Engine {
    /// Validate the config and compile every rule. All problems are reported at once.
    pub fn new(
        config: &OxcruiseConfig,
        external: Option<Arc<dyn ExternalRules>>,
    ) -> Result<Engine, Vec<String>> {
        let mut errors = vec![];
        let mut plans: Vec<Plan> = vec![];
        let ignore_cycles = config.options.cycles.ignore_type_only;
        let mut next_external = 0u32;

        // Built-in and plugin rules from `rules`.
        for (name, entry) in &config.rules {
            let entry = match entry_of(name, entry) {
                Ok(e) => e,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            let declarative_named = config
                .forbid
                .iter()
                .chain(&config.allow)
                .any(|r| &r.name == name)
                || config.require.iter().any(|r| &r.name == name)
                || name == NOT_IN_ALLOWED;
            if declarative_named {
                continue; // severity override for a declarative rule; handled below
            }
            let source = match RuleEnum::builtin(name, entry.options.clone()) {
                Some(Ok(_)) => Source::Builtin,
                Some(Err(e)) => {
                    errors.push(e);
                    continue;
                }
                None => match &external {
                    Some(x) if x.rule_exists(name) => {
                        next_external += 1;
                        Source::External(ExternalRuleId(next_external - 1))
                    }
                    _ => {
                        errors.push(unknown_rule_message(name));
                        continue;
                    }
                },
            };
            plans.push(Plan {
                name: name.clone(),
                source,
                base: Some(entry),
                overrides: vec![],
            });
        }

        // Declarative rules.
        let level_override = |name: &str, default: Level| -> Level {
            config
                .rules
                .get(name)
                .and_then(|e| e.parts().ok())
                .map_or(default, |(l, _)| l)
        };
        for r in &config.forbid {
            match CompiledRule::forbid(r, ignore_cycles) {
                Ok(c) => plans.push(Plan {
                    name: r.name.clone(),
                    source: Source::Declarative(Arc::new(c)),
                    base: Some(Entry {
                        level: level_override(&r.name, r.severity),
                        options: serde_json::Value::Null,
                    }),
                    overrides: vec![],
                }),
                Err(e) => errors.push(e),
            }
        }
        for r in &config.require {
            match CompiledRule::require(r, ignore_cycles) {
                Ok(c) => plans.push(Plan {
                    name: r.name.clone(),
                    source: Source::Declarative(Arc::new(c)),
                    base: Some(Entry {
                        level: level_override(&r.name, r.severity),
                        options: serde_json::Value::Null,
                    }),
                    overrides: vec![],
                }),
                Err(e) => errors.push(e),
            }
        }
        if !config.allow.is_empty() {
            let mut entries = vec![];
            for r in &config.allow {
                match CompiledRule::allow(r, ignore_cycles) {
                    Ok(c) => entries.push(c),
                    Err(e) => errors.push(e),
                }
            }
            let level = level_override(NOT_IN_ALLOWED, config.allow[0].severity);
            plans.push(Plan {
                name: NOT_IN_ALLOWED.into(),
                source: Source::AllowSet(Arc::new(AllowSet {
                    entries,
                    severity: level.to_severity().unwrap_or(Severity::Ignore),
                })),
                base: Some(Entry {
                    level,
                    options: serde_json::Value::Null,
                }),
                overrides: vec![],
            });
        }

        // Overrides.
        let mut override_files = vec![];
        for (i, o) in config.overrides.iter().enumerate() {
            let pats: Vec<PathPattern> = o
                .files
                .iter()
                .map(|f| PathPattern::Glob(f.clone()))
                .collect();
            match PathMatcher::new(&pats) {
                Ok(m) => override_files.push(m),
                Err(e) => {
                    errors.push(format!("overrides[{i}]: {e}"));
                    override_files.push(PathMatcher::default());
                }
            }
            for (name, entry) in &o.rules {
                let entry = match entry_of(name, entry) {
                    Ok(e) => e,
                    Err(e) => {
                        errors.push(format!("overrides[{i}]: {e}"));
                        continue;
                    }
                };
                match plans.iter_mut().find(|p| &p.name == name) {
                    Some(plan) => {
                        match &plan.source {
                            Source::Builtin => {
                                if let Some(Err(e)) = RuleEnum::builtin(name, entry.options.clone())
                                {
                                    errors.push(format!("overrides[{i}]: {e}"));
                                    continue;
                                }
                            }
                            Source::External(_) => {
                                errors.push(format!("overrides[{i}]: plugin rule `{name}` cannot be overridden per file yet"));
                                continue;
                            }
                            _ => {}
                        }
                        plan.overrides.push((i, entry));
                    }
                    None => {
                        // Rule only enabled inside this override.
                        match RuleEnum::builtin(name, entry.options.clone()) {
                            Some(Ok(_)) => plans.push(Plan {
                                name: name.clone(),
                                source: Source::Builtin,
                                base: None,
                                overrides: vec![(i, entry)],
                            }),
                            Some(Err(e)) => errors.push(format!("overrides[{i}]: {e}")),
                            None => errors
                                .push(format!("overrides[{i}]: {}", unknown_rule_message(name))),
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            Ok(Engine {
                plans,
                override_files,
                external,
                report_dead_rules: config.options.report_dead_rules,
            })
        } else {
            Err(errors)
        }
    }

    pub fn run(&self, graph: &Graph, root: &Path, workspace: &[WorkspacePackage]) -> RunResult {
        let mut warnings = vec![];
        let mut resolved: Vec<Resolved> = vec![];
        let mut externals: Vec<ExternalRuleSpec> = vec![];

        for plan in &self.plans {
            if let Source::External(id) = &plan.source {
                let base = plan
                    .base
                    .as_ref()
                    .expect("external rules have a base entry");
                if let Some(sev) = base.level.to_severity() {
                    externals.push(ExternalRuleSpec {
                        id: *id,
                        name: plan.name.clone(),
                        severity: sev,
                        options: base.options.clone(),
                    });
                }
                continue;
            }
            self.resolve_plan(plan, graph, &mut resolved);
        }

        let wanted = resolved
            .iter()
            .fold(Analyses::empty(), |a, r| a | r.rule.requirements());
        let analyses = AnalysisCache::compute(graph, wanted);
        let shared = Shared {
            graph,
            root,
            workspace,
            analyses: &analyses,
        };

        let mut violations: Vec<Violation> = resolved
            .par_iter()
            .flat_map_iter(|r| run_rule(r, &shared))
            .collect();

        if !externals.is_empty() {
            match &self.external {
                Some(x) => match x.run(graph, workspace, &externals) {
                    Ok(found) => {
                        for f in found {
                            let spec = &externals[externals
                                .iter()
                                .position(|s| s.id == f.rule)
                                .expect("known external rule")];
                            let mut v = match f.target {
                                ExternalTarget::Module(m) => {
                                    Violation::module(&spec.name, spec.severity, m, f.message)
                                }
                                ExternalTarget::Edge(e) => {
                                    let edge = graph.edge(e);
                                    Violation {
                                        rule: spec.name.clone(),
                                        severity: spec.severity,
                                        from: edge.from,
                                        to: Some(edge.to),
                                        edge: Some(e),
                                        message: f.message,
                                        loc: edge.loc,
                                        path: vec![],
                                    }
                                }
                            };
                            v.rule = spec.name.clone();
                            violations.push(v);
                        }
                    }
                    Err(e) => warnings.push(format!("plugin rules failed: {e}")),
                },
                None => warnings
                    .push("plugin rules are configured but no plugin host is available".into()),
            }
        }

        // Suppressions, then ordering.
        let before = violations.len();
        violations.retain(|v| !is_suppressed(graph, v));
        let suppressed = before - violations.len();
        violations.sort_by(|a, b| {
            let key = |v: &Violation| {
                (
                    rank(v.severity),
                    graph.module(v.from).path.clone(),
                    v.loc,
                    v.to.map(|t| graph.module(t).path.clone()),
                    v.rule.clone(),
                    v.message.clone(),
                )
            };
            key(a).cmp(&key(b))
        });

        if self.report_dead_rules {
            for plan in &self.plans {
                if let Source::Declarative(c) = &plan.source {
                    warnings.extend(c.dead_reasons());
                }
                if let Source::AllowSet(a) = &plan.source {
                    for e in &a.entries {
                        warnings.extend(e.dead_reasons());
                    }
                }
            }
        }
        RunResult {
            violations,
            suppressed,
            warnings,
        }
    }

    /// Turn a plan into runnable rules: one per distinct effective (severity, options),
    /// each limited to the modules it applies to.
    fn resolve_plan(&self, plan: &Plan, graph: &Graph, out: &mut Vec<Resolved>) {
        let make = |opts: &serde_json::Value| -> Option<RuleEnum> {
            match &plan.source {
                Source::Builtin => RuleEnum::builtin(&plan.name, opts.clone()).and_then(Result::ok),
                Source::Declarative(c) => Some(RuleEnum::Declarative(c.clone())),
                Source::AllowSet(a) => Some(RuleEnum::AllowSet(a.clone())),
                Source::External(_) => None,
            }
        };
        if plan.overrides.is_empty() {
            let Some(base) = &plan.base else { return };
            if let (Some(sev), Some(rule)) = (base.level.to_severity(), make(&base.options)) {
                out.push(Resolved {
                    name: plan.name.clone(),
                    rule,
                    severity: sev,
                    modules: None,
                });
            }
            return;
        }

        // Effective entry per module: base, then each matching override in order.
        let n = graph.module_count();
        let mut groups: BTreeMap<String, (Entry, BitSet)> = BTreeMap::new();
        for i in 0..n {
            let path = &graph.module(ModuleId(i as u32)).path;
            let mut eff = plan.base.clone();
            for (oi, entry) in &plan.overrides {
                if self.override_files[*oi].is_match(path) {
                    eff = Some(entry.clone());
                }
            }
            let Some(eff) = eff else { continue };
            if eff.level == Level::Off {
                continue;
            }
            let key = format!("{:?}|{}", eff.level, eff.options);
            groups
                .entry(key)
                .or_insert_with(|| (eff, BitSet::new(n)))
                .1
                .insert(i);
        }
        for (_, (entry, set)) in groups {
            if let (Some(sev), Some(rule)) = (entry.level.to_severity(), make(&entry.options)) {
                out.push(Resolved {
                    name: plan.name.clone(),
                    rule,
                    severity: sev,
                    modules: Some(set),
                });
            }
        }
    }
}

fn unknown_rule_message(name: &str) -> String {
    if name.contains('/') {
        format!("unknown rule `{name}`: plugin rules need `jsPlugins` and the npm package")
    } else {
        let near = crate::registry::builtin_rules()
            .into_iter()
            .map(|r| r.name)
            .min_by_key(|r| edit_distance(r, name))
            .filter(|r| edit_distance(r, name) <= 3);
        match near {
            Some(r) => format!("unknown rule `{name}`; did you mean `{r}`?"),
            None => format!("unknown rule `{name}`"),
        }
    }
}

fn edit_distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}

struct Resolved {
    name: String,
    rule: RuleEnum,
    severity: Severity,
    modules: Option<BitSet>,
}

fn run_rule(r: &Resolved, shared: &Shared<'_>) -> Vec<Violation> {
    let g = shared.graph;
    let in_scope = |m: ModuleId| r.modules.as_ref().is_none_or(|s| s.contains(m.idx()));
    let new_cx = || RuleCtx::new(shared, &r.name, r.severity);
    let join = |mut a: Vec<Violation>, mut b: Vec<Violation>| {
        a.append(&mut b);
        a
    };

    let mut out: Vec<Violation> = (0..g.module_count() as u32)
        .into_par_iter()
        .map(ModuleId)
        .filter(|&m| in_scope(m))
        .fold(new_cx, |cx, m| {
            r.rule.check_module(m, &cx);
            cx
        })
        .map(RuleCtx::into_violations)
        .reduce(Vec::new, join);

    let edges: Vec<Violation> = (0..g.edge_count() as u32)
        .into_par_iter()
        .map(oxcruise_core::EdgeId)
        .filter(|&e| in_scope(g.edge(e).from))
        .fold(new_cx, |cx, e| {
            r.rule.check_edge(e, &cx);
            cx
        })
        .map(RuleCtx::into_violations)
        .reduce(Vec::new, join);
    out.extend(edges);

    if r.modules.is_none() {
        let cx = new_cx();
        r.rule.check_graph(&cx);
        out.extend(cx.into_violations());
    }
    out
}

fn is_suppressed(g: &Graph, v: &Violation) -> bool {
    let covers = |list: &[String]| list.iter().any(|r| r == "*" || *r == v.rule);
    v.edge.is_some_and(|e| covers(&g.edge(e).suppressed)) || covers(&g.module(v.from).suppressed)
}
