use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use oxcruise_config::{Category, Level, OxcruiseConfig, RuleEntry};
use oxcruise_core::{Graph, Loc, ModuleId, Severity, Violation};
use oxcruise_graph::PathMatcher;
use oxcruise_graph::analyses::BitSet;
use oxcruise_resolve::WorkspacePackage;
use rayon::prelude::*;

use crate::declarative::CompiledRule;
use crate::external::{ExternalRuleId, ExternalRuleSpec, ExternalRules, ExternalTarget};
use crate::meta::{Analyses, AnalysisCache, RuleCtx, Shared};
use crate::registry::{RuleEnum, builtin_rules};

#[derive(Clone)]
struct Entry {
    level: Level,
    options: serde_json::Value,
}

enum Source {
    Builtin,
    Declarative(Arc<CompiledRule>),
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
}

#[derive(Debug, Default)]
pub struct RunResult {
    /// Sorted: severity, then location.
    pub violations: Vec<Violation>,
    /// Violations silenced by `oxcruise-ignore-*` comments.
    pub suppressed: usize,
    pub warnings: Vec<String>,
    /// Problems that make the result untrustworthy (a plugin crashed). The run must fail.
    pub fatal: Vec<String>,
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
        let mut next_external = 0u32;

        // Built-in rules switched on by their category (default options)...
        let mut levels: BTreeMap<Category, Level> = Category::ALL
            .into_iter()
            .map(|c| (c, c.default_level()))
            .collect();
        levels.extend(config.categories.clone());
        for info in builtin_rules() {
            let level = levels[&info.category];
            if level != Level::Off {
                plans.push(Plan {
                    name: info.name.to_string(),
                    source: Source::Builtin,
                    base: Some(Entry {
                        level,
                        options: serde_json::Value::Null,
                    }),
                    overrides: vec![],
                });
            }
        }

        // ... then `rules`, which win over the category.
        for (name, entry) in &config.rules {
            let entry = match entry_of(name, entry) {
                Ok(e) => e,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
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
            match plans.iter_mut().find(|p| &p.name == name) {
                Some(plan) => plan.base = Some(entry),
                None => plans.push(Plan {
                    name: name.clone(),
                    source,
                    base: Some(entry),
                    overrides: vec![],
                }),
            }
        }

        // Forbidden dependencies. Their level comes only from their own `severity`.
        let builtin_names: Vec<&str> = builtin_rules().into_iter().map(|r| r.name).collect();
        for r in &config.forbid {
            let taken = builtin_names.contains(&r.name.as_str())
                || plans
                    .iter()
                    .any(|p| p.name == r.name && !matches!(p.source, Source::Builtin));
            if taken {
                errors.push(format!(
                    "forbid rule `{}`: the name is already used by another rule",
                    r.name
                ));
                continue;
            }
            match CompiledRule::forbid(r) {
                Ok(c) => plans.push(Plan {
                    name: r.name.clone(),
                    source: Source::Declarative(Arc::new(c)),
                    base: Some(Entry {
                        level: r.severity,
                        options: serde_json::Value::Null,
                    }),
                    overrides: vec![],
                }),
                Err(e) => errors.push(e),
            }
        }

        // Overrides.
        let mut override_files = vec![];
        for (i, o) in config.overrides.iter().enumerate() {
            match PathMatcher::new(&o.files) {
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
            })
        } else {
            Err(errors)
        }
    }

    pub fn run(&self, graph: &Graph, root: &Path, workspace: &[WorkspacePackage]) -> RunResult {
        let mut warnings = vec![];
        let mut fatal = vec![];
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
                    Err(e) => fatal.push(format!("plugin rules failed: {e}")),
                },
                None => warnings
                    .push("plugin rules are configured but no plugin host is available".into()),
            }
        }

        // Suppressions, then one violation per cycle, then ordering.
        let (cycle_violations, mut violations): (Vec<_>, Vec<_>) =
            violations.into_iter().partition(is_cycle_violation);
        let mut suppressed = 0;
        violations.retain(|v| {
            let hidden = is_suppressed(graph, v);
            suppressed += usize::from(hidden);
            !hidden
        });
        let (cycles, suppressed_cycles) = dedupe_cycles(graph, cycle_violations);
        suppressed += suppressed_cycles;
        violations.extend(cycles);
        // module ids follow path order (see `Graph::build`), so comparing ids compares paths
        violations.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));

        for plan in &self.plans {
            if let Source::Declarative(c) = &plan.source {
                warnings.extend(c.dead_reasons());
            }
        }
        RunResult {
            violations,
            suppressed,
            warnings,
            fatal,
        }
    }

    /// Turn a plan into runnable rules: one per distinct effective (severity, options),
    /// each limited to the modules it applies to.
    fn resolve_plan(&self, plan: &Plan, graph: &Graph, out: &mut Vec<Resolved>) {
        let make = |opts: &serde_json::Value| -> Option<RuleEnum> {
            match &plan.source {
                Source::Builtin => RuleEnum::builtin(&plan.name, opts.clone()).and_then(Result::ok),
                Source::Declarative(c) => Some(RuleEnum::Declarative(c.clone())),
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

#[allow(clippy::type_complexity)]
fn sort_key(v: &Violation) -> (u8, ModuleId, Option<Loc>, Option<ModuleId>, &str, &str) {
    (
        rank(v.severity),
        v.from,
        v.loc,
        v.to,
        v.rule.as_str(),
        v.message.as_str(),
    )
}

/// A violation on an edge that carries the cycle it belongs to.
fn is_cycle_violation(v: &Violation) -> bool {
    v.edge.is_some() && !v.path.is_empty()
}

/// Every edge of a cycle is reported by its own violation, all describing the same cycle.
/// Keep one per distinct cycle (the one starting at the alphabetically first module). A cycle
/// is suppressed when *any* of its edges is: annotate one import to accept the whole cycle.
/// Returns the kept violations and how many cycles were suppressed.
fn dedupe_cycles(g: &Graph, violations: Vec<Violation>) -> (Vec<Violation>, usize) {
    let mut groups: BTreeMap<(String, Vec<u32>), Vec<Violation>> = BTreeMap::new();
    for v in violations {
        let mut ids: Vec<u32> = v.path.iter().map(|s| s.module.0).collect();
        // module ids follow path order, so the minimum id is the alphabetically first module
        let start = ids
            .iter()
            .enumerate()
            .min_by_key(|(_, id)| **id)
            .map_or(0, |(i, _)| i);
        ids.rotate_left(start);
        groups.entry((v.rule.clone(), ids)).or_default().push(v);
    }
    let (mut kept, mut suppressed) = (vec![], 0);
    for (_, group) in groups {
        if group.iter().any(|v| is_suppressed(g, v)) {
            suppressed += 1;
        } else if let Some(v) = group.into_iter().min_by_key(|v| (v.from, v.edge)) {
            kept.push(v);
        }
    }
    (kept, suppressed)
}

fn is_suppressed(g: &Graph, v: &Violation) -> bool {
    let covers = |list: &[String]| list.iter().any(|r| r == "*" || *r == v.rule);
    v.edge.is_some_and(|e| covers(&g.edge(e).suppressed)) || covers(&g.module(v.from).suppressed)
}
