//! Compiles `forbid` / `allow` / `require` rules from config into checks over the graph.

use std::sync::atomic::{AtomicUsize, Ordering};

use oxcruise_config::{
    DeclarativeRule, FromCondition, FromSpec, OneOrMany, PathSpec, Relation, RelationSpec,
    RequireRule, ToCondition, ToSpec, ViaCondition,
};
use oxcruise_core::{CycleStep, DepKinds, Edge, EdgeId, Graph, Module, ModuleId, ModuleKind};
use oxcruise_graph::analyses::{reach_from, shortest_cycle};

use crate::meta::{Analyses, RuleCtx};
use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Forbid,
    /// One `allow` entry; the engine groups them into `not-in-allowed`.
    Allow,
    Require,
}

#[derive(Debug)]
pub struct CompiledRule {
    pub name: String,
    pub kind: Kind,
    pub comment: Option<String>,
    from: From,
    to: To,
    ignore_type_only_cycles: bool,
    from_hits: AtomicUsize,
    to_path_hits: AtomicUsize,
}

#[derive(Debug, Default)]
struct From {
    path: Option<PathPatterns>,
    path_not: Option<PathPatterns>,
    orphan: Option<bool>,
    dependents_lt: Option<u32>,
    dependents_gt: Option<u32>,
}

#[derive(Debug, Default)]
struct To {
    path: Option<PathPatterns>,
    path_not: Option<PathPatterns>,
    relation: Option<(Vec<Relation>, Vec<Relation>)>,
    orphan: Option<bool>,
    dependency_types: DepKinds,
    dependency_types_not: DepKinds,
    more_than_one_dependency_type: Option<bool>,
    could_not_resolve: Option<bool>,
    dynamic: Option<bool>,
    type_only: Option<bool>,
    exotically_required: Option<bool>,
    circular: Option<bool>,
    via: Option<Via>,
    via_only: Option<Via>,
    reachable: bool,
    dependents_lt: Option<u32>,
    dependents_gt: Option<u32>,
}

#[derive(Debug, Default)]
struct Via {
    path: Option<PathPatterns>,
    path_not: Option<PathPatterns>,
    dependency_types: DepKinds,
    dependency_types_not: DepKinds,
    type_only: Option<bool>,
    dynamic: Option<bool>,
}

impl Via {
    fn has_module_part(&self) -> bool {
        self.path.is_some() || self.path_not.is_some()
    }
    fn has_edge_part(&self) -> bool {
        !self.dependency_types.is_empty()
            || !self.dependency_types_not.is_empty()
            || self.type_only.is_some()
            || self.dynamic.is_some()
    }
    fn module_matches(&self, m: &Module, caps: &Captures) -> bool {
        self.path.as_ref().is_none_or(|p| p.is_match(&m.path, caps))
            && !self
                .path_not
                .as_ref()
                .is_some_and(|p| p.is_match(&m.path, caps))
    }
    fn edge_matches(&self, e: &Edge) -> bool {
        (self.dependency_types.is_empty() || e.kinds.intersects(self.dependency_types))
            && !e.kinds.intersects(self.dependency_types_not)
            && self.type_only.is_none_or(|b| e.is_type_only() == b)
            && self
                .dynamic
                .is_none_or(|b| e.kinds.contains(DepKinds::DYNAMIC_IMPORT) == b)
    }
}

fn pats(spec: &Option<OneOrMany<PathSpec>>, role: Role) -> Result<Option<PathPatterns>, String> {
    spec.as_ref()
        .map(|s| PathPatterns::new(&s.to_vec(), role))
        .transpose()
}

fn kinds_of(names: &[String], rule: &str) -> Result<DepKinds, String> {
    let mut k = DepKinds::empty();
    for n in names {
        k |= DepKinds::from_depcruise_name(n)
            .ok_or_else(|| format!("rule `{rule}`: unknown dependency type `{n}`"))?;
    }
    Ok(k)
}

fn compile_via(v: &ViaCondition, rule: &str) -> Result<Via, String> {
    Ok(Via {
        path: pats(&v.path, Role::Referencing)?,
        path_not: pats(&v.path_not, Role::Referencing)?,
        dependency_types: kinds_of(&v.dependency_types, rule)?,
        dependency_types_not: kinds_of(&v.dependency_types_not, rule)?,
        type_only: v.type_only,
        dynamic: v.dynamic,
    })
}

fn compile_from(spec: &Option<FromSpec>) -> Result<From, String> {
    let c = match spec {
        None => FromCondition::default(),
        Some(FromSpec::Path(p)) => FromCondition {
            path: Some(OneOrMany::One(PathSpec::glob(p))),
            ..Default::default()
        },
        Some(FromSpec::Cond(c)) => c.clone(),
    };
    Ok(From {
        path: pats(&c.path, Role::Capturing)?,
        path_not: pats(&c.path_not, Role::Referencing)?,
        orphan: c.orphan,
        dependents_lt: c.number_of_dependents_less_than,
        dependents_gt: c.number_of_dependents_more_than,
    })
}

fn compile_to(
    spec: &Option<ToSpec>,
    rule: &str,
    ignore_type_only_cycles: bool,
) -> Result<To, String> {
    let _ = ignore_type_only_cycles;
    let c: ToCondition = match spec {
        None => ToCondition::default(),
        Some(ToSpec::Path(p)) => ToCondition {
            path: Some(OneOrMany::One(PathSpec::glob(p))),
            ..Default::default()
        },
        Some(ToSpec::Cond(c)) => (**c).clone(),
    };
    let relation = c.relation.as_ref().map(|r| match r {
        RelationSpec::Any(v) => (v.clone(), vec![]),
        RelationSpec::Spec(o) => (o.any.clone(), o.not.clone()),
    });
    let reachable = match c.reachable {
        Some(true) => true,
        Some(false) => {
            return Err(format!(
                "rule `{rule}`: `reachable: false` is not supported yet"
            ));
        }
        None => false,
    };
    let to = To {
        path: pats(&c.path, Role::Referencing)?,
        path_not: pats(&c.path_not, Role::Referencing)?,
        relation,
        orphan: c.orphan,
        dependency_types: kinds_of(&c.dependency_types, rule)?,
        dependency_types_not: kinds_of(&c.dependency_types_not, rule)?,
        more_than_one_dependency_type: c.more_than_one_dependency_type,
        could_not_resolve: c.could_not_resolve,
        dynamic: c.dynamic,
        type_only: c.type_only,
        exotically_required: c.exotically_required,
        circular: c.circular,
        via: c.via.as_ref().map(|v| compile_via(v, rule)).transpose()?,
        via_only: c
            .via_only
            .as_ref()
            .map(|v| compile_via(v, rule))
            .transpose()?,
        reachable,
        dependents_lt: c.number_of_dependents_less_than,
        dependents_gt: c.number_of_dependents_more_than,
    };
    if (to.via.is_some() || to.via_only.is_some()) && to.circular != Some(true) {
        return Err(format!(
            "rule `{rule}`: `via`/`viaOnly` need `circular: true`"
        ));
    }
    if to.reachable
        && (!to.dependency_types.is_empty()
            || !to.dependency_types_not.is_empty()
            || to.circular.is_some()
            || to.type_only.is_some()
            || to.dynamic.is_some()
            || to.could_not_resolve.is_some())
    {
        return Err(format!(
            "rule `{rule}`: `reachable` can only be combined with path, pathNot, orphan and dependents conditions"
        ));
    }
    Ok(to)
}

impl CompiledRule {
    pub fn forbid(r: &DeclarativeRule, ignore_type_only_cycles: bool) -> Result<Self, String> {
        Self::compile(r, Kind::Forbid, ignore_type_only_cycles)
    }

    pub fn allow(r: &DeclarativeRule, ignore_type_only_cycles: bool) -> Result<Self, String> {
        Self::compile(r, Kind::Allow, ignore_type_only_cycles)
    }

    fn compile(
        r: &DeclarativeRule,
        kind: Kind,
        ignore_type_only_cycles: bool,
    ) -> Result<Self, String> {
        Ok(Self {
            name: r.name.clone(),
            kind,
            comment: r.comment.clone(),
            from: compile_from(&r.from).map_err(|e| format!("rule `{}`: {e}", r.name))?,
            to: compile_to(&r.to, &r.name, ignore_type_only_cycles).map_err(|e| {
                if e.starts_with("rule `") {
                    e
                } else {
                    format!("rule `{}`: {e}", r.name)
                }
            })?,
            ignore_type_only_cycles,
            from_hits: AtomicUsize::new(0),
            to_path_hits: AtomicUsize::new(0),
        })
    }

    pub fn require(r: &RequireRule, ignore_type_only_cycles: bool) -> Result<Self, String> {
        Ok(Self {
            name: r.name.clone(),
            kind: Kind::Require,
            comment: r.comment.clone(),
            from: compile_from(&Some(r.module.clone()))
                .map_err(|e| format!("rule `{}`: {e}", r.name))?,
            to: compile_to(&Some(r.to.clone()), &r.name, ignore_type_only_cycles)?,
            ignore_type_only_cycles,
            from_hits: AtomicUsize::new(0),
            to_path_hits: AtomicUsize::new(0),
        })
    }

    pub fn requirements(&self) -> Analyses {
        let mut a = Analyses::empty();
        if self.to.circular.is_some() {
            a |= if self.ignore_type_only_cycles {
                Analyses::CYCLES_RUNTIME
            } else {
                Analyses::CYCLES
            };
        }
        if self.from.dependents_lt.is_some()
            || self.from.dependents_gt.is_some()
            || self.to.dependents_lt.is_some()
            || self.to.dependents_gt.is_some()
        {
            a |= Analyses::DEPENDENTS;
        }
        a
    }

    /// A forbid rule with no `to` conditions that is about a module on its own: orphans, dependents.
    fn is_module_rule(&self) -> bool {
        self.kind == Kind::Forbid
            && !self.to.has_any()
            && (self.from.orphan.is_some()
                || self.from.dependents_lt.is_some()
                || self.from.dependents_gt.is_some())
    }

    // ---- from side -------------------------------------------------------------------------

    fn match_from(&self, m: ModuleId, cx: &RuleCtx<'_>) -> Option<Captures> {
        let g = cx.graph();
        let module = cx.module(m);
        let caps = match &self.from.path {
            Some(p) => p.capture(&module.path)?,
            None => Captures::default(),
        };
        if self
            .from
            .path_not
            .as_ref()
            .is_some_and(|p| p.is_match(&module.path, &caps))
        {
            return None;
        }
        if let Some(want) = self.from.orphan
            && is_orphan(g, m) != want
        {
            return None;
        }
        if !dependents_ok(cx, m, self.from.dependents_lt, self.from.dependents_gt) {
            return None;
        }
        Some(caps)
    }

    // ---- to side ---------------------------------------------------------------------------

    /// Does the edge satisfy the `to` conditions? Returns the cycle/path that explains it, if any.
    fn match_to(&self, e: EdgeId, caps: &Captures, cx: &RuleCtx<'_>) -> Option<Vec<CycleStep>> {
        let g = cx.graph();
        let edge = cx.edge(e);
        let to = cx.module(edge.to);
        let from = cx.module(edge.from);
        let t = &self.to;

        if let Some(p) = &t.path {
            if !p.is_match(&to.path, caps) {
                return None;
            }
            self.to_path_hits.fetch_add(1, Ordering::Relaxed);
        }
        if t.path_not
            .as_ref()
            .is_some_and(|p| p.is_match(&to.path, caps))
        {
            return None;
        }
        if let Some((any, not)) = &t.relation {
            if !any.is_empty() && !any.iter().any(|r| relation_holds(*r, from, to)) {
                return None;
            }
            if not.iter().any(|r| relation_holds(*r, from, to)) {
                return None;
            }
        }
        if let Some(want) = t.orphan
            && is_orphan(g, edge.to) != want
        {
            return None;
        }
        if !t.dependency_types.is_empty() && !edge.kinds.intersects(t.dependency_types) {
            return None;
        }
        if edge.kinds.intersects(t.dependency_types_not) {
            return None;
        }
        if let Some(want) = t.more_than_one_dependency_type
            && (edge.kinds & NPM_SECTIONS).bits().count_ones().gt(&1) != want
        {
            return None;
        }
        if let Some(want) = t.could_not_resolve
            && (to.kind == ModuleKind::Unresolved) != want
        {
            return None;
        }
        if t.dynamic
            .is_some_and(|b| edge.kinds.contains(DepKinds::DYNAMIC_IMPORT) != b)
            || t.type_only.is_some_and(|b| edge.is_type_only() != b)
            || t.exotically_required
                .is_some_and(|b| edge.kinds.contains(DepKinds::EXOTIC_REQUIRE) != b)
        {
            return None;
        }
        if !dependents_ok(cx, edge.to, t.dependents_lt, t.dependents_gt) {
            return None;
        }

        let mut path = vec![];
        if let Some(want) = t.circular {
            let ignore = self.ignore_type_only_cycles;
            let scc = cx.analyses().scc(ignore);
            let keep = move |x: &Edge| !(ignore && x.is_type_only());
            let cycle = shortest_cycle(g, scc, e, &keep);
            if cycle.is_some() != want {
                return None;
            }
            if let Some(cycle) = cycle {
                if !via_ok(t, &cycle, caps, cx) {
                    return None;
                }
                path = cycle;
            }
        }
        Some(path)
    }

    // ---- entry points used by the engine --------------------------------------------------------

    pub fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        let module = cx.module(m);
        if module.kind != ModuleKind::Source {
            return;
        }
        match self.kind {
            Kind::Forbid if self.is_module_rule() => {
                if self.match_from(m, cx).is_some() {
                    self.from_hits.fetch_add(1, Ordering::Relaxed);
                    cx.report_module(m, self.message("module is forbidden by this rule", cx, &[]));
                }
            }
            Kind::Forbid if self.to.reachable => self.check_reachable(m, cx),
            Kind::Require => {
                let Some(caps) = self.match_from(m, cx) else {
                    return;
                };
                self.from_hits.fetch_add(1, Ordering::Relaxed);
                let g = cx.graph();
                let satisfied = g
                    .out_edge_ids(m)
                    .any(|e| self.match_to(e, &caps, cx).is_some());
                if !satisfied {
                    cx.report_module(
                        m,
                        self.message("module lacks a required dependency", cx, &[]),
                    );
                }
            }
            _ => {}
        }
    }

    pub fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        if self.kind != Kind::Forbid || self.is_module_rule() || self.to.reachable {
            return;
        }
        let edge = cx.edge(e);
        let Some(caps) = self.match_from(edge.from, cx) else {
            return;
        };
        self.from_hits.fetch_add(1, Ordering::Relaxed);
        if let Some(path) = self.match_to(e, &caps, cx) {
            let msg = self.message("forbidden dependency", cx, &path);
            cx.report_edge_with_path(e, msg, path);
        }
    }

    /// For `allow` entries: does this edge match the entry?
    pub fn allows(&self, e: EdgeId, cx: &RuleCtx<'_>) -> bool {
        let edge = cx.edge(e);
        match self.match_from(edge.from, cx) {
            Some(caps) => self.match_to(e, &caps, cx).is_some(),
            None => false,
        }
    }

    fn check_reachable(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        let Some(caps) = self.match_from(m, cx) else {
            return;
        };
        self.from_hits.fetch_add(1, Ordering::Relaxed);
        let g = cx.graph();
        let reach = reach_from(g, &[m], &|e: &Edge| {
            !e.is_type_only() || !self.ignore_type_only_cycles
        });
        for t in reach
            .reached
            .iter()
            .map(|i| ModuleId(i as u32))
            .filter(|&t| t != m)
        {
            let to = cx.module(t);
            if to.kind != ModuleKind::Source && to.kind != ModuleKind::Stub {
                continue;
            }
            if self
                .to
                .path
                .as_ref()
                .is_some_and(|p| !p.is_match(&to.path, &caps))
                || self
                    .to
                    .path_not
                    .as_ref()
                    .is_some_and(|p| p.is_match(&to.path, &caps))
                || self.to.orphan.is_some_and(|o| is_orphan(g, t) != o)
                || !dependents_ok(cx, t, self.to.dependents_lt, self.to.dependents_gt)
            {
                continue;
            }
            self.to_path_hits.fetch_add(1, Ordering::Relaxed);
            let path = reach.path_to(g, t).unwrap_or_default();
            let msg = self
                .comment
                .clone()
                .unwrap_or_else(|| format!("`{}` is reachable from here", to.path));
            let mut v = oxcruise_core::Violation::module(cx.rule, cx.severity, m, msg);
            v.to = Some(t);
            v.path = path;
            // reachable violations are module-level; push through the public API
            cx.push(v);
        }
    }

    /// What is wrong, in words. The reporter already prints `from -> to`, so the
    /// message is the rule's own comment when it has one.
    fn message(&self, what: &str, cx: &RuleCtx<'_>, path: &[CycleStep]) -> String {
        if let Some(c) = &self.comment {
            return c.clone();
        }
        if path.len() > 1 && self.to.circular == Some(true) {
            let mut names: Vec<&str> = path
                .iter()
                .map(|s| cx.module(s.module).path.as_str())
                .collect();
            names.push(names[0]);
            return format!("circular dependency: {}", names.join(" → "));
        }
        what.to_string()
    }

    /// Reasons this rule can never fire, for `reportDeadRules`.
    pub fn dead_reasons(&self) -> Vec<String> {
        let mut out = vec![];
        let has_from = self.from.path.is_some()
            || self.from.orphan.is_some()
            || self.from.dependents_lt.is_some()
            || self.from.dependents_gt.is_some();
        if has_from && self.from_hits.load(Ordering::Relaxed) == 0 {
            out.push(format!(
                "rule `{}`: its `from` conditions matched no module",
                self.name
            ));
        } else if self.to.path.is_some() && self.to_path_hits.load(Ordering::Relaxed) == 0 {
            out.push(format!(
                "rule `{}`: its `to.path` matched no dependency of the modules matching `from`",
                self.name
            ));
        }
        out
    }
}

impl To {
    fn has_any(&self) -> bool {
        self.path.is_some()
            || self.path_not.is_some()
            || self.relation.is_some()
            || self.orphan.is_some()
            || !self.dependency_types.is_empty()
            || !self.dependency_types_not.is_empty()
            || self.more_than_one_dependency_type.is_some()
            || self.could_not_resolve.is_some()
            || self.dynamic.is_some()
            || self.type_only.is_some()
            || self.exotically_required.is_some()
            || self.circular.is_some()
            || self.reachable
            || self.dependents_lt.is_some()
            || self.dependents_gt.is_some()
    }
}

const NPM_SECTIONS: DepKinds = DepKinds::NPM
    .union(DepKinds::NPM_DEV)
    .union(DepKinds::NPM_PEER)
    .union(DepKinds::NPM_OPTIONAL)
    .union(DepKinds::NPM_BUNDLED);

fn is_orphan(g: &Graph, m: ModuleId) -> bool {
    g.module(m).kind == ModuleKind::Source && g.in_degree(m) == 0 && g.out_degree(m) == 0
}

fn dependents_ok(cx: &RuleCtx<'_>, m: ModuleId, lt: Option<u32>, gt: Option<u32>) -> bool {
    if lt.is_none() && gt.is_none() {
        return true;
    }
    let n = cx.analyses().dependents_of(m);
    lt.is_none_or(|x| n < x) && gt.is_none_or(|x| n > x)
}

fn via_ok(t: &To, cycle: &[CycleStep], caps: &Captures, cx: &RuleCtx<'_>) -> bool {
    // `via`: some module other than the importer (so `to` counts), or some edge.
    // `viaOnly`: every module of the cycle, importer included, and every edge. That
    // keeps the verdict the same whichever edge of the cycle is being looked at.
    let others: Vec<&Module> = cycle.iter().skip(1).map(|s| cx.module(s.module)).collect();
    let all: Vec<&Module> = cycle.iter().map(|s| cx.module(s.module)).collect();
    let edges: Vec<&Edge> = cycle
        .iter()
        .filter_map(|s| s.edge)
        .map(|e| cx.edge(e))
        .collect();
    if let Some(v) = &t.via {
        let m_ok = !v.has_module_part() || others.iter().any(|m| v.module_matches(m, caps));
        let e_ok = !v.has_edge_part() || edges.iter().any(|e| v.edge_matches(e));
        if !(v.has_module_part() || v.has_edge_part()) || !(m_ok && e_ok) {
            return false;
        }
    }
    if let Some(v) = &t.via_only {
        let m_ok = all.iter().all(|m| v.module_matches(m, caps));
        let e_ok = edges.iter().all(|e| v.edge_matches(e));
        if !(m_ok && e_ok) {
            return false;
        }
    }
    true
}

fn dir_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn parent_of(dir: &str) -> &str {
    dir.rsplit_once('/').map_or("", |(d, _)| d)
}

pub(crate) fn relation_holds(r: Relation, from: &Module, to: &Module) -> bool {
    let (fd, td) = (dir_of(&from.path), dir_of(&to.path));
    let below = |dir: &str, base: &str| {
        base.is_empty() || dir == base || dir.starts_with(&format!("{base}/"))
    };
    match r {
        Relation::SameFolder => fd == td,
        Relation::DescendantOfFrom => {
            !fd.is_empty() && td != fd && below(td, fd) || fd.is_empty() && !td.is_empty()
        }
        Relation::DescendantOfFromParent => below(td, parent_of(fd)),
        Relation::Sibling => td != fd && parent_of(td) == parent_of(fd),
        Relation::SamePackage => from.package.is_some() && from.package == to.package,
        Relation::CrossPackage => {
            from.package.is_some() && to.package.is_some() && from.package != to.package
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxcruise_core::ModuleKind;

    fn m(path: &str, pkg: Option<u32>) -> Module {
        let mut m = Module::new(path, ModuleKind::Source);
        m.package = pkg;
        m
    }

    #[test]
    fn relations() {
        let from = m("src/features/a/x.ts", Some(0));
        let cases = [
            (Relation::SameFolder, "src/features/a/y.ts", true),
            (Relation::SameFolder, "src/features/a/deep/y.ts", false),
            (Relation::DescendantOfFrom, "src/features/a/deep/y.ts", true),
            (Relation::DescendantOfFrom, "src/features/a/y.ts", false),
            (
                Relation::DescendantOfFromParent,
                "src/features/b/y.ts",
                true,
            ),
            (Relation::DescendantOfFromParent, "src/other/y.ts", false),
            (Relation::Sibling, "src/features/b/y.ts", true),
            (Relation::Sibling, "src/features/a/y.ts", false),
            (Relation::Sibling, "src/features/b/deep/y.ts", false),
        ];
        for (rel, to, want) in cases {
            assert_eq!(
                relation_holds(rel, &from, &m(to, Some(0))),
                want,
                "{rel:?} {to}"
            );
        }
        assert!(relation_holds(
            Relation::SamePackage,
            &from,
            &m("a.ts", Some(0))
        ));
        assert!(relation_holds(
            Relation::CrossPackage,
            &from,
            &m("a.ts", Some(1))
        ));
        assert!(!relation_holds(
            Relation::CrossPackage,
            &from,
            &m("a.ts", None)
        ));
        // root-level files: everything is below the (empty) parent
        assert!(relation_holds(
            Relation::DescendantOfFromParent,
            &m("a.ts", None),
            &m("x/b.ts", None)
        ));
    }
}

/// All `allow` entries, evaluated as one rule: a dependency matching none of them is a violation.
#[derive(Debug)]
pub struct AllowSet {
    pub entries: Vec<CompiledRule>,
    pub severity: oxcruise_core::Severity,
}

pub const NOT_IN_ALLOWED: &str = "not-in-allowed";

impl AllowSet {
    pub fn requirements(&self) -> Analyses {
        self.entries
            .iter()
            .fold(Analyses::empty(), |a, e| a | e.requirements())
    }

    pub fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        if self.entries.iter().any(|r| r.allows(e, cx)) {
            return;
        }
        let edge = cx.edge(e);
        cx.report_edge(
            e,
            format!(
                "{} → {} is not allowed by any `allow` rule",
                cx.module(edge.from).path,
                cx.module(edge.to).path
            ),
        );
    }
}
