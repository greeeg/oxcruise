//! Compiles `forbid` rules from config into checks over the graph.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use oxcruise_config::{ForbidRule, Kind, OneOrMany, Relation, ToCondition, ToSpec};
use oxcruise_core::{DepKinds, EdgeId, Module, ModuleId, ModuleKind};
use oxcruise_graph::analyses::{BitSet, Reach, can_reach};

use crate::meta::{Analyses, Hooks, RuleCtx};
use crate::pattern::{Captures, PathPatterns, Role};

/// A `forbid` rule as configured: immutable, so an engine can run it on any number of graphs.
#[derive(Debug)]
pub struct ForbidSpec {
    pub name: String,
    message: Option<String>,
    from: Option<PathPatterns>,
    to: To,
}

/// One run of a [`ForbidSpec`]: what the run has seen, for the warnings about dead rules and
/// the reachability shortcut.
#[derive(Debug)]
pub struct CompiledRule {
    spec: Arc<ForbidSpec>,
    from_hits: AtomicBool,
    to_path_hits: AtomicBool,
    /// For `reachable` rules whose `to` does not refer to `from` captures: the modules that
    /// reach a target at all, which is the same for every `from`. Filled on first use.
    can_reach_target: OnceLock<Option<BitSet>>,
}

thread_local! {
    /// Buffers of the reachability walks of this thread, reused from one module to the next.
    static REACH: RefCell<Option<Reach>> = const { RefCell::new(None) };
}

#[derive(Debug, Default)]
struct To {
    path: Option<PathPatterns>,
    kinds: Option<Vec<Kind>>,
    type_only: Option<bool>,
    dynamic: Option<bool>,
    relation: Option<Vec<Relation>>,
    reachable: bool,
}

fn many<T: Clone>(v: &OneOrMany<T>) -> Vec<T> {
    v.to_vec()
}

fn compile_to(spec: &Option<ToSpec>, rule: &str) -> Result<To, String> {
    let c = match spec {
        None => ToCondition::default(),
        Some(ToSpec::Paths(p)) => ToCondition {
            path: Some(p.clone()),
            ..Default::default()
        },
        Some(ToSpec::Cond(c)) => c.clone(),
    };
    let reachable = match c.reachable {
        Some(true) => true,
        Some(false) => {
            return Err(format!(
                "rule `{rule}`: `reachable: false` is not supported yet"
            ));
        }
        None => false,
    };
    if reachable && (c.type_only.is_some() || c.dynamic.is_some() || c.relation.is_some()) {
        return Err(format!(
            "rule `{rule}`: `reachable` can only be combined with `path` and `kind`"
        ));
    }
    Ok(To {
        path: c
            .path
            .as_ref()
            .map(|p| PathPatterns::new(&many(p), Role::Referencing))
            .transpose()?,
        kinds: c.kind.as_ref().map(many),
        type_only: c.type_only,
        dynamic: c.dynamic,
        relation: c.relation.as_ref().map(many),
        reachable,
    })
}

impl ForbidSpec {
    pub fn forbid(r: &ForbidRule) -> Result<Self, String> {
        let err = |e: String| {
            if e.starts_with("rule `") {
                e
            } else {
                format!("rule `{}`: {e}", r.name)
            }
        };
        Ok(Self {
            name: r.name.clone(),
            message: r.message.clone(),
            from: r
                .from
                .as_ref()
                .map(|p| PathPatterns::new(&many(p), Role::Capturing))
                .transpose()
                .map_err(err)?,
            to: compile_to(&r.to, &r.name).map_err(err)?,
        })
    }
}

impl CompiledRule {
    /// A fresh run of `spec`, with nothing seen yet.
    pub fn new(spec: &Arc<ForbidSpec>) -> Self {
        Self {
            spec: spec.clone(),
            from_hits: AtomicBool::new(false),
            to_path_hits: AtomicBool::new(false),
            can_reach_target: OnceLock::new(),
        }
    }

    pub fn requirements(&self) -> Analyses {
        Analyses::empty()
    }

    /// Every rule works module by module: `from` is matched once per module, and only the
    /// modules that match look at their dependencies.
    pub fn hooks(&self) -> Hooks {
        Hooks::MODULE
    }

    // ---- from side -------------------------------------------------------------------------

    /// The captures of `from` on `m`, or `None` if `m` is not a source of this rule.
    fn match_from(&self, m: ModuleId, cx: &RuleCtx<'_>) -> Option<Captures> {
        match &self.spec.from {
            Some(p) => p.capture(&cx.module(m).path),
            None => Some(Captures::default()),
        }
    }

    /// Set a hit flag without writing to a cache line that is already set: every worker
    /// thread reads these, and only the first hit needs a store.
    fn hit(flag: &AtomicBool) {
        if !flag.load(Ordering::Relaxed) {
            flag.store(true, Ordering::Relaxed);
        }
    }

    // ---- to side ---------------------------------------------------------------------------

    fn match_to(&self, e: EdgeId, caps: &Captures, cx: &RuleCtx<'_>) -> bool {
        let edge = cx.edge(e);
        let to = cx.module(edge.to);
        let from = cx.module(edge.from);
        let t = &self.spec.to;

        if let Some(p) = &t.path {
            if !p.is_match(&to.path, caps) {
                return false;
            }
            Self::hit(&self.to_path_hits);
        }
        if let Some(kinds) = &t.kinds
            && !kinds.contains(&to.kind)
        {
            return false;
        }
        if let Some(any) = &t.relation
            && !any.iter().any(|r| relation_holds(*r, from, to))
        {
            return false;
        }
        t.dynamic
            .is_none_or(|b| edge.kinds.contains(DepKinds::DYNAMIC_IMPORT) == b)
            && t.type_only.is_none_or(|b| edge.is_type_only() == b)
    }

    // ---- entry points used by the engine ---------------------------------------------------

    pub fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        if self.spec.to.reachable {
            if cx.module(m).kind == ModuleKind::Local {
                self.check_reachable(m, cx);
            }
            return;
        }
        let edges = cx.graph().out_edge_ids(m);
        if edges.len() == 0 {
            return;
        }
        let Some(caps) = self.match_from(m, cx) else {
            return;
        };
        Self::hit(&self.from_hits);
        for e in edges {
            if self.match_to(e, &caps, cx) {
                cx.report_edge(e, self.message("forbidden dependency"));
            }
        }
    }

    /// Does `t` count as a target of a `reachable` rule, given the captures of `from`?
    fn is_target(&self, t: &Module, caps: &Captures) -> bool {
        let kind = t.kind;
        let wanted = match &self.spec.to.kinds {
            Some(kinds) => kinds.contains(&kind),
            // like a plain dependency target: project files and packages, not built-ins
            None => matches!(kind, Kind::Local | Kind::Package),
        };
        wanted
            && self
                .spec
                .to
                .path
                .as_ref()
                .is_none_or(|p| p.is_match(&t.path, caps))
    }

    /// The modules that reach a target. Only when the targets do not depend on `from`.
    fn can_reach_target(&self, cx: &RuleCtx<'_>) -> Option<&BitSet> {
        self.can_reach_target
            .get_or_init(|| {
                if self
                    .spec
                    .to
                    .path
                    .as_ref()
                    .is_some_and(|p| p.refers_to_captures())
                {
                    return None;
                }
                let g = cx.graph();
                let none = Captures::default();
                let mut targets = BitSet::new(g.module_count());
                for (i, t) in g.modules().iter().enumerate() {
                    if self.is_target(t, &none) {
                        targets.insert(i);
                    }
                }
                Some(can_reach(g, &targets))
            })
            .as_ref()
    }

    fn check_reachable(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        let Some(caps) = self.match_from(m, cx) else {
            return;
        };
        Self::hit(&self.from_hits);
        // most modules reach no target: skip the walk for them
        if self
            .can_reach_target(cx)
            .is_some_and(|s| !s.contains(m.idx()))
        {
            return;
        }
        let g = cx.graph();
        REACH.with_borrow_mut(|slot| {
            let reach = match slot {
                Some(r) if r.reached.len() == g.module_count() => r,
                _ => slot.insert(Reach::new(g.module_count())),
            };
            reach.run(g, &[m], &|_| true);
            for t in reach
                .reached
                .iter()
                .map(|i| ModuleId(i as u32))
                .filter(|&t| t != m)
            {
                let to = cx.module(t);
                if !self.is_target(to, &caps) {
                    continue;
                }
                Self::hit(&self.to_path_hits);
                let msg = self
                    .spec
                    .message
                    .clone()
                    .unwrap_or_else(|| format!("`{}` is reachable from here", to.path));
                let mut v = oxcruise_core::Violation::module(cx.rule, cx.severity, m, msg);
                v.to = Some(t);
                v.path = reach.path_to(g, t).unwrap_or_default();
                cx.push(v);
            }
        });
    }

    /// What is wrong, in words. The reporter already prints `from -> to`, so the
    /// message is the rule's own `message` when it has one.
    fn message(&self, what: &str) -> String {
        self.spec
            .message
            .clone()
            .unwrap_or_else(|| what.to_string())
    }

    /// Reasons this rule can never fire: a typo in a glob, or a path that does not exist.
    pub fn dead_reasons(&self) -> Vec<String> {
        let mut out = vec![];
        if self.spec.from.is_some() && !self.from_hits.load(Ordering::Relaxed) {
            out.push(format!(
                "rule `{}`: its `from` matched no module",
                self.spec.name
            ));
        } else if self.spec.to.path.is_some() && !self.to_path_hits.load(Ordering::Relaxed) {
            out.push(format!(
                "rule `{}`: its `to.path` matched no dependency of the modules matching `from`",
                self.spec.name
            ));
        }
        out
    }
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
        base.is_empty()
            || dir == base
            || dir
                .strip_prefix(base)
                .is_some_and(|rest| rest.starts_with('/'))
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

    fn m(path: &str, pkg: Option<u32>) -> Module {
        let mut m = Module::new(path, ModuleKind::Local);
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
