//! Compiles `forbid` rules from config into checks over the graph.

use std::sync::atomic::{AtomicUsize, Ordering};

use oxcruise_config::{ForbidRule, Kind, OneOrMany, Relation, ToCondition, ToSpec};
use oxcruise_core::{DepKinds, Edge, EdgeId, Module, ModuleId, ModuleKind};
use oxcruise_graph::analyses::reach_from;

use crate::meta::{Analyses, RuleCtx};
use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug)]
pub struct CompiledRule {
    pub name: String,
    message: Option<String>,
    from: Option<PathPatterns>,
    to: To,
    from_hits: AtomicUsize,
    to_path_hits: AtomicUsize,
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

/// What a module is, for the `kind` condition.
fn kind_of(m: &Module) -> Kind {
    match m.kind {
        ModuleKind::Source => Kind::Local,
        // resolved but not parsed: with node_modules always leaves, that is a package file
        ModuleKind::Stub => Kind::Package,
        ModuleKind::Core => Kind::Builtin,
        ModuleKind::Unresolved => Kind::Unresolved,
    }
}

impl CompiledRule {
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
            from_hits: AtomicUsize::new(0),
            to_path_hits: AtomicUsize::new(0),
        })
    }

    pub fn requirements(&self) -> Analyses {
        Analyses::empty()
    }

    // ---- from side -------------------------------------------------------------------------

    fn match_from(&self, m: ModuleId, cx: &RuleCtx<'_>) -> Option<Captures> {
        match &self.from {
            Some(p) => p.capture(&cx.module(m).path),
            None => Some(Captures::default()),
        }
    }

    // ---- to side ---------------------------------------------------------------------------

    fn match_to(&self, e: EdgeId, caps: &Captures, cx: &RuleCtx<'_>) -> bool {
        let edge = cx.edge(e);
        let to = cx.module(edge.to);
        let from = cx.module(edge.from);
        let t = &self.to;

        if let Some(p) = &t.path {
            if !p.is_match(&to.path, caps) {
                return false;
            }
            self.to_path_hits.fetch_add(1, Ordering::Relaxed);
        }
        if let Some(kinds) = &t.kinds
            && !kinds.contains(&kind_of(to))
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
        if self.to.reachable && cx.module(m).kind == ModuleKind::Source {
            self.check_reachable(m, cx);
        }
    }

    pub fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        if self.to.reachable {
            return;
        }
        let edge = cx.edge(e);
        let Some(caps) = self.match_from(edge.from, cx) else {
            return;
        };
        self.from_hits.fetch_add(1, Ordering::Relaxed);
        if self.match_to(e, &caps, cx) {
            cx.report_edge(e, self.message("forbidden dependency"));
        }
    }

    fn check_reachable(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        let Some(caps) = self.match_from(m, cx) else {
            return;
        };
        self.from_hits.fetch_add(1, Ordering::Relaxed);
        let g = cx.graph();
        let reach = reach_from(g, &[m], &|_: &Edge| true);
        for t in reach
            .reached
            .iter()
            .map(|i| ModuleId(i as u32))
            .filter(|&t| t != m)
        {
            let to = cx.module(t);
            let kind = kind_of(to);
            let wanted = match &self.to.kinds {
                Some(kinds) => kinds.contains(&kind),
                // like a plain dependency target: project files and packages, not built-ins
                None => matches!(kind, Kind::Local | Kind::Package),
            };
            if !wanted
                || self
                    .to
                    .path
                    .as_ref()
                    .is_some_and(|p| !p.is_match(&to.path, &caps))
            {
                continue;
            }
            self.to_path_hits.fetch_add(1, Ordering::Relaxed);
            let msg = self
                .message
                .clone()
                .unwrap_or_else(|| format!("`{}` is reachable from here", to.path));
            let mut v = oxcruise_core::Violation::module(cx.rule, cx.severity, m, msg);
            v.to = Some(t);
            v.path = reach.path_to(g, t).unwrap_or_default();
            cx.push(v);
        }
    }

    /// What is wrong, in words. The reporter already prints `from -> to`, so the
    /// message is the rule's own `message` when it has one.
    fn message(&self, what: &str) -> String {
        self.message.clone().unwrap_or_else(|| what.to_string())
    }

    /// Reasons this rule can never fire: a typo in a glob, or a path that does not exist.
    pub fn dead_reasons(&self) -> Vec<String> {
        let mut out = vec![];
        if self.from.is_some() && self.from_hits.load(Ordering::Relaxed) == 0 {
            out.push(format!(
                "rule `{}`: its `from` matched no module",
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
