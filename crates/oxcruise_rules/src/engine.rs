use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use oxcruise_config::{Category, Level, OxcruiseConfig, RuleEntry};
use oxcruise_core::{
    DeclaredManifest, FileTree, Graph, Loc, ModuleId, Subject, Violation, ViolationLevel,
};
use oxcruise_graph::PathMatcher;
use oxcruise_graph::analyses::{BitSet, ExportUsage};
use oxcruise_resolve::WorkspacePackage;
use rayon::prelude::*;

use crate::conventions::ConventionFilter;
use crate::declarative::{CompiledRule, ForbidSpec};
use crate::entries::EntryInputs;
use crate::external::{ExternalRuleId, ExternalRuleSpec, ExternalRules, ExternalTarget};
use crate::meta::{Analyses, AnalysisCache, Hooks, RuleCtx, Shared};
use crate::registry::{RuleEnum, builtin_rules};

/// The setting of a rule in `rules`, in its category or in `forbid`.
#[derive(Clone)]
struct Entry {
    level: Level,
    options: serde_json::Value,
}

/// The setting of a rule in `overrides[].rules`. Without a level, the override keeps the level
/// that the rule has for the module.
#[derive(Clone)]
struct Override {
    level: Option<Level>,
    options: serde_json::Value,
}

enum Source {
    Builtin,
    Declarative(Arc<ForbidSpec>),
    External(ExternalRuleId),
}

struct Plan {
    name: String,
    source: Source,
    base: Option<Entry>,
    /// `(override index, entry)`; the override's `files` decide which modules it affects.
    overrides: Vec<(usize, Override)>,
}

pub struct Engine {
    plans: Vec<Plan>,
    override_files: Vec<PathMatcher>,
    external: Option<Arc<dyn ExternalRules>>,
    /// `wanted()`, which parses every rule's options; cleared when they change.
    wanted: OnceLock<Analyses>,
}

#[derive(Debug, Default)]
pub struct RunResult {
    /// Sorted: level, then location.
    pub violations: Vec<Violation>,
    /// Violations silenced by `oxcruise-ignore-*` comments.
    pub suppressed: usize,
    pub warnings: Vec<String>,
    /// Problems that make the result untrustworthy (a plugin crashed). The run must fail.
    pub fatal: Vec<String>,
    /// Which exports are used (entries, conventions and plugins decide what is public), when the
    /// graph carries symbols. Independent of any rule's options.
    pub usage: Option<ExportUsage>,
}

impl RunResult {
    pub fn count(&self, s: ViolationLevel) -> usize {
        self.violations.iter().filter(|v| v.level == s).count()
    }
}

fn rank(s: ViolationLevel) -> u8 {
    match s {
        ViolationLevel::Error => 0,
        ViolationLevel::Warn => 1,
    }
}

/// An entry of `rules`: an object without a level means `error`.
fn entry_of(e: &RuleEntry) -> Entry {
    let (level, options) = e.parts();
    Entry {
        level: level.unwrap_or(Level::Error),
        options,
    }
}

fn override_of(e: &RuleEntry) -> Override {
    let (level, options) = e.parts();
    Override { level, options }
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
            let entry = entry_of(entry);
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

        // Forbidden dependencies. Their level comes only from their own `level`.
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
            match ForbidSpec::forbid(r) {
                Ok(c) => plans.push(Plan {
                    name: r.name.clone(),
                    source: Source::Declarative(Arc::new(c)),
                    base: Some(Entry {
                        level: r.level,
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
                let entry = override_of(entry);
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
                wanted: OnceLock::new(),
            })
        } else {
            Err(errors)
        }
    }

    /// Set option `key` of built-in rule `rule` to `value`, in the rule's configuration and in
    /// every override of it, so a command-line flag wins over the config file. Rules that are
    /// off stay off.
    pub fn set_option(&mut self, rule: &str, key: &str, value: serde_json::Value) {
        self.wanted = OnceLock::new();
        let patch = |options: &mut serde_json::Value| {
            if options.is_null() {
                *options = serde_json::json!({});
            }
            if let Some(o) = options.as_object_mut() {
                o.insert(key.to_string(), value.clone());
            }
        };
        for plan in self
            .plans
            .iter_mut()
            .filter(|p| p.name == rule && matches!(p.source, Source::Builtin))
        {
            plan.base.iter_mut().for_each(|e| patch(&mut e.options));
            plan.overrides
                .iter_mut()
                .for_each(|(_, e)| patch(&mut e.options));
        }
    }

    /// Turn on the `production` option of every built-in rule that has one (`--production`).
    pub fn set_production(&mut self) {
        let names: Vec<String> = self
            .plans
            .iter()
            .filter(|p| {
                matches!(p.source, Source::Builtin) && RuleEnum::has_production_option(&p.name)
            })
            .map(|p| p.name.clone())
            .collect();
        for name in names {
            self.set_option(&name, "production", serde_json::json!(true));
        }
    }

    /// Whole-graph analyses the enabled rules need, with the options they are configured with.
    fn wanted(&self) -> Analyses {
        *self.wanted.get_or_init(|| self.compute_wanted())
    }

    fn compute_wanted(&self) -> Analyses {
        let mut wanted = Analyses::empty();
        for plan in &self.plans {
            if !matches!(plan.source, Source::Builtin) {
                continue;
            }
            // an override without a level is on when anything else switches the rule on
            let on = |l: Option<Level>| l.is_some_and(|l| l != Level::Off);
            let some_on = on(plan.base.as_ref().map(|b| b.level))
                || plan.overrides.iter().any(|(_, o)| on(o.level));
            let options = plan
                .base
                .iter()
                .filter(|e| e.level != Level::Off)
                .map(|e| &e.options)
                .chain(
                    plan.overrides
                        .iter()
                        .filter(|(_, o)| o.level.map_or(some_on, |l| l != Level::Off))
                        .map(|(_, o)| &o.options),
                );
            for options in options {
                if let Some(Ok(rule)) = RuleEnum::builtin(&plan.name, options.clone()) {
                    wanted |= rule.requirements();
                }
            }
        }
        wanted
    }

    /// Whether `cruise` must extract exports and the names each import uses.
    pub fn wants_symbols(&self) -> bool {
        self.wanted().contains(Analyses::SYMBOLS)
    }

    /// Whether `cruise` must read the `package.json` of every module.
    pub fn wants_manifests(&self) -> bool {
        self.wanted()
            .intersects(Analyses::MANIFESTS | Analyses::ENTRIES | Analyses::DEP_PEERS)
    }

    /// Whether `cruise` must record every file of the project, not only the source files.
    pub fn wants_file_tree(&self) -> bool {
        self.wanted().contains(Analyses::FILE_TREE)
    }

    /// Whether `cruise` must also read the peer dependencies of the declared packages.
    pub fn wants_peers(&self) -> bool {
        self.wanted().contains(Analyses::DEP_PEERS)
    }

    /// Check `graph`. `file_tree` is every file of the project, for the rules that ask for it
    /// (`wants_file_tree`). `symbols` says whether the graph was built with exports and the names
    /// each import uses (`cruise` extracted them).
    pub fn run(
        &self,
        graph: &Graph,
        root: &Path,
        workspace: &[WorkspacePackage],
        manifests: &[DeclaredManifest],
        file_tree: Option<&FileTree>,
        symbols: bool,
    ) -> RunResult {
        let mut warnings = vec![];
        let mut fatal = vec![];
        let mut resolved: Vec<Resolved<'_>> = vec![];
        let mut externals: Vec<ExternalRuleSpec> = vec![];

        // which modules each `overrides[].files` matches, once for all plans
        let override_sets: Vec<BitSet> = if self.plans.iter().any(|p| !p.overrides.is_empty()) {
            self.override_files
                .iter()
                .map(|matcher| {
                    let hits: Vec<u32> = (0..graph.module_count() as u32)
                        .into_par_iter()
                        .filter(|&i| matcher.is_match(&graph.module(ModuleId(i)).path))
                        .collect();
                    let mut set = BitSet::new(graph.module_count());
                    for i in hits {
                        set.insert(i as usize);
                    }
                    set
                })
                .collect()
        } else {
            vec![]
        };

        // what each `forbid` rule saw in this run only, not in earlier runs of the engine
        let runs: Vec<Option<Arc<CompiledRule>>> = self
            .plans
            .iter()
            .map(|p| match &p.source {
                Source::Declarative(spec) => Some(Arc::new(CompiledRule::new(spec))),
                _ => None,
            })
            .collect();

        for (plan, run) in self.plans.iter().zip(&runs) {
            if let Source::External(id) = &plan.source {
                let base = plan
                    .base
                    .as_ref()
                    .expect("external rules have a base entry");
                if let Some(sev) = base.level.to_violation_level() {
                    externals.push(ExternalRuleSpec {
                        id: *id,
                        name: plan.name.clone(),
                        level: sev,
                        options: base.options.clone(),
                    });
                }
                continue;
            }
            self.resolve_plan(plan, run.as_ref(), graph, &override_sets, &mut resolved);
        }

        let mut wanted = resolved
            .iter()
            .fold(Analyses::empty(), |a, r| a | r.rule.requirements());
        if !externals.is_empty() {
            // the graph handed to plugins carries both
            wanted |= Analyses::CYCLES | Analyses::DEPENDENTS;
        }
        // what each rule that needs entry points takes as one, and what plugins add
        let conventions: Vec<ConventionFilter> = resolved
            .iter()
            .filter(|r| r.rule.requirements().contains(Analyses::ENTRIES))
            .map(|r| r.rule.conventions())
            .collect();
        let plugin_entries = self.external.as_ref().map_or(vec![], |x| x.entries());
        let mut analyses = AnalysisCache::compute(
            graph,
            wanted,
            &EntryInputs {
                manifests,
                conventions: &conventions,
                plugin_entries: &plugin_entries,
            },
            symbols,
        );
        let shared = Shared {
            graph,
            root,
            workspace,
            manifests,
            file_tree,
            analyses: &analyses,
            warnings: Mutex::default(),
        };

        let mut violations: Vec<Violation> = resolved
            .par_iter()
            .flat_map_iter(|r| run_rule(r, &shared))
            .collect();

        let mut notes = shared.warnings.into_inner().expect("poisoned");
        notes.sort();
        notes.dedup();
        warnings.extend(notes);
        let usage = analyses.usage.take();

        if !externals.is_empty() {
            match &self.external {
                Some(x) => match x.run(graph, workspace, &externals, usage.as_ref(), &analyses) {
                    Ok(found) => {
                        let specs: BTreeMap<ExternalRuleId, &ExternalRuleSpec> =
                            externals.iter().map(|s| (s.id, s)).collect();
                        for f in found {
                            let spec = specs[&f.rule];
                            let mut v = match f.target {
                                ExternalTarget::Module(m) => {
                                    Violation::module(&spec.name, spec.level, m, f.message)
                                }
                                ExternalTarget::Export(m, i) => {
                                    let mut v =
                                        Violation::module(&spec.name, spec.level, m, f.message);
                                    v.loc = Some(graph.module(m).exports[i as usize].loc);
                                    v.subject = Subject::Export(i);
                                    v
                                }
                                ExternalTarget::Edge(e) => {
                                    let edge = graph.edge(e);
                                    Violation {
                                        rule: spec.name.clone(),
                                        level: spec.level,
                                        from: edge.from,
                                        to: Some(edge.to),
                                        edge: Some(e),
                                        message: f.message,
                                        loc: edge.loc,
                                        path: vec![],
                                        subject: Subject::Module,
                                        fix: None,
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

        for run in runs.iter().flatten() {
            warnings.extend(run.dead_reasons());
        }
        RunResult {
            violations,
            suppressed,
            warnings,
            fatal,
            usage,
        }
    }

    /// Turn a plan into runnable rules: one per distinct effective (level, options),
    /// each limited to the modules it applies to.
    fn resolve_plan<'e>(
        &'e self,
        plan: &'e Plan,
        run: Option<&Arc<CompiledRule>>,
        graph: &Graph,
        override_sets: &[BitSet],
        out: &mut Vec<Resolved<'e>>,
    ) {
        let make = |opts: &serde_json::Value| -> Option<RuleEnum> {
            match &plan.source {
                Source::Builtin => RuleEnum::builtin(&plan.name, opts.clone()).and_then(Result::ok),
                Source::Declarative(_) => run.map(|c| RuleEnum::Declarative(c.clone())),
                Source::External(_) => None,
            }
        };
        if plan.overrides.is_empty() {
            let Some(base) = &plan.base else { return };
            if let (Some(sev), Some(rule)) = (base.level.to_violation_level(), make(&base.options))
            {
                out.push(Resolved {
                    name: plan.name.clone(),
                    rule,
                    level: sev,
                    modules: None,
                    paths: None,
                });
            }
            return;
        }

        // Effective setting per module: base, then each matching override in order. The last
        // matching override decides the options, and the last one with a level decides the level.
        // Modules are grouped by (the setting that gave the options, level), not one by one.
        let n = graph.module_count();
        let base_level = plan.base.as_ref().map_or(Level::Off, |b| b.level);
        let mut by_slot: BTreeMap<(usize, u8), BitSet> = BTreeMap::new();
        for i in 0..n {
            let (slot, level) =
                self.effective(plan, base_level, |oi| override_sets[oi].contains(i));
            if let Some(code) = level_code(level) {
                by_slot
                    .entry((slot, code))
                    .or_insert_with(|| BitSet::new(n))
                    .insert(i);
            }
        }
        // A rule that checks paths needs a variant for every setting a path can have, also those
        // that no module has: a folder can match an override that no module matches.
        let path_rule = make(&serde_json::Value::Null)
            .is_some_and(|r| r.requirements().contains(Analyses::FILE_TREE));
        if path_rule {
            for slot in 0..=plan.overrides.len() {
                let codes: &[u8] = match slot {
                    0 => &[],
                    k => match plan.overrides[k - 1].1.level {
                        Some(l) => &[level_code(l).unwrap_or(2)][..],
                        None => &[0, 1],
                    },
                };
                let base_code = level_code(base_level);
                for &code in codes.iter().chain(base_code.iter().filter(|_| slot == 0)) {
                    if code < 2 {
                        by_slot
                            .entry((slot, code))
                            .or_insert_with(|| BitSet::new(n));
                    }
                }
            }
        }
        // Slots with the same effective setting become one rule.
        type Group<'p> = (Level, &'p serde_json::Value, BitSet, Vec<(usize, u8)>);
        let mut groups: BTreeMap<String, Group<'_>> = BTreeMap::new();
        for ((slot, code), set) in by_slot {
            let level = if code == 1 { Level::Warn } else { Level::Error };
            let options = match slot {
                0 => match &plan.base {
                    Some(b) => &b.options,
                    None => continue,
                },
                k => &plan.overrides[k - 1].1.options,
            };
            let key = format!("{level:?}|{options}");
            match groups.entry(key) {
                std::collections::btree_map::Entry::Occupied(mut o) => {
                    o.get_mut().2.union_with(&set);
                    o.get_mut().3.push((slot, code));
                }
                std::collections::btree_map::Entry::Vacant(v) => {
                    v.insert((level, options, set, vec![(slot, code)]));
                }
            }
        }
        for (_, (level, options, set, slots)) in groups {
            if let (Some(level), Some(rule)) = (level.to_violation_level(), make(options)) {
                out.push(Resolved {
                    name: plan.name.clone(),
                    rule,
                    level,
                    modules: Some(set),
                    paths: path_rule.then_some(PathScope {
                        engine: self,
                        plan,
                        slots,
                    }),
                });
            }
        }
    }

    /// The setting a module (or path) has: the index of the setting that gives the options
    /// (0 is the base, `k + 1` is override `k`) and the level. `matches` says whether the
    /// `files` of an override match.
    fn effective(
        &self,
        plan: &Plan,
        base_level: Level,
        matches: impl Fn(usize) -> bool,
    ) -> (usize, Level) {
        let (mut slot, mut level) = (0, base_level);
        for (k, (oi, o)) in plan.overrides.iter().enumerate() {
            if matches(*oi) {
                slot = k + 1;
                level = o.level.unwrap_or(level);
            }
        }
        (slot, level)
    }
}

/// `0` for `error`, `1` for `warn`, `None` for `off`.
fn level_code(level: Level) -> Option<u8> {
    match level {
        Level::Off => None,
        Level::Warn => Some(1),
        Level::Error => Some(0),
    }
}

/// Which paths a rule that reports paths applies to: those whose effective setting is one of
/// `slots`. The `files` of the overrides match the path itself, not a module.
struct PathScope<'e> {
    engine: &'e Engine,
    plan: &'e Plan,
    slots: Vec<(usize, u8)>,
}

impl PathScope<'_> {
    fn contains(&self, path: &str) -> bool {
        let base_level = self.plan.base.as_ref().map_or(Level::Off, |b| b.level);
        let (slot, level) = self.engine.effective(self.plan, base_level, |oi| {
            self.engine.override_files[oi].is_match(path)
        });
        level_code(level).is_some_and(|code| self.slots.contains(&(slot, code)))
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

struct Resolved<'e> {
    name: String,
    rule: RuleEnum,
    level: ViolationLevel,
    modules: Option<BitSet>,
    /// Set for a rule that reports paths, when `overrides` change its setting.
    paths: Option<PathScope<'e>>,
}

fn run_rule(r: &Resolved<'_>, shared: &Shared<'_>) -> Vec<Violation> {
    let g = shared.graph;
    let in_scope = |m: ModuleId| r.modules.as_ref().is_none_or(|s| s.contains(m.idx()));
    let new_cx = || RuleCtx::new(shared, &r.name, r.level);
    let join = |mut a: Vec<Violation>, mut b: Vec<Violation>| {
        a.append(&mut b);
        a
    };

    let hooks = r.rule.hooks();
    let mut out: Vec<Violation> = vec![];
    if hooks.contains(Hooks::MODULE) {
        out = (0..g.module_count() as u32)
            .into_par_iter()
            .map(ModuleId)
            .filter(|&m| in_scope(m))
            .fold(new_cx, |cx, m| {
                r.rule.check_module(m, &cx);
                cx
            })
            .map(RuleCtx::into_violations)
            .reduce(Vec::new, join);
    }

    if hooks.contains(Hooks::EDGE) {
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
    }

    if hooks.contains(Hooks::GRAPH) {
        // Whole-graph findings belong to the module they are reported on.
        let cx = new_cx();
        r.rule.check_graph(&cx);
        out.extend(
            cx.into_violations()
                .into_iter()
                .filter(|v| match &v.subject {
                    // a path belongs to the setting its own path has
                    Subject::Path(path) => r.paths.as_ref().is_none_or(|p| p.contains(path)),
                    _ => in_scope(v.from),
                }),
        );
    }
    out
}

#[allow(clippy::type_complexity)]
fn sort_key(v: &Violation) -> (u8, ModuleId, Option<Loc>, Option<ModuleId>, &str, &str) {
    (
        rank(v.level),
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
    // a cycle between folders (no `to`) is not the cycle between modules that has the same steps
    let mut groups: BTreeMap<(String, bool, Vec<u32>), Vec<Violation>> = BTreeMap::new();
    for v in violations {
        let mut ids: Vec<u32> = v.path.iter().map(|s| s.module.0).collect();
        // module ids follow path order, so the minimum id is the alphabetically first module
        let start = ids
            .iter()
            .enumerate()
            .min_by_key(|(_, id)| **id)
            .map_or(0, |(i, _)| i);
        ids.rotate_left(start);
        groups
            .entry((v.rule.clone(), v.to.is_none(), ids))
            .or_default()
            .push(v);
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
    match &v.subject {
        Subject::File(_) => return false,
        // only a module has a comment: `oxcruise-ignore-file` in the module that the path names
        Subject::Path(path) => {
            return g
                .find(path)
                .is_some_and(|m| covers(&g.module(m).suppressed));
        }
        _ => {}
    }
    let module = g.module(v.from);
    v.edge.is_some_and(|e| covers(&g.edge(e).suppressed))
        || matches!(v.subject, Subject::Export(i) if covers(&module.exports[i as usize].suppressed))
        || covers(&module.suppressed)
}
