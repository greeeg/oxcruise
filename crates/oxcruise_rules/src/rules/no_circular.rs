use std::sync::OnceLock;

use oxcruise_core::{Edge, EdgeId, Graph};
use oxcruise_graph::analyses::{BitSet, Scc, shortest_cycle, strongly_connected};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Analyses, Rule, RuleCtx, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NoCircularConfig {
    /// Also report cycles that exist only through type-only imports (`import type`).
    /// By default only cycles that exist at runtime are reported.
    pub include_type_only: bool,
    /// Globs of modules to leave out before looking for cycles, so no cycle goes through them.
    pub ignore: Vec<String>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports dependencies that are part of a circular chain of imports and
    /// shows the shortest cycle through each of them.
    ///
    /// ### Why is this bad?
    ///
    /// Cycles make modules impossible to load, test or reason about in
    /// isolation, and can cause `undefined` values at runtime.
    ///
    /// ### Options
    ///
    /// `includeTypeOnly`: also count type-only imports, which are erased at runtime. Off by
    /// default, so a cycle that exists only through `import type` is not reported.
    ///
    /// `ignore`: globs of modules removed from the graph before cycles are searched, for example
    /// a barrel file everything legitimately goes through. Use `overrides` to scope the rule to
    /// some files instead.
    NoCircular, category = correctness, version = "0.1.0", config = NoCircularConfig
}

/// Cycles of a graph with some modules removed; computed on first use.
#[derive(Debug)]
struct Pruned {
    scc: Scc,
    removed: BitSet,
}

#[derive(Debug, Default)]
pub struct NoCircular {
    include_type_only: bool,
    ignore: Option<PathPatterns>,
    pruned: OnceLock<Pruned>,
}

impl NoCircular {
    fn keeps(&self, edge: &Edge, removed: Option<&BitSet>) -> bool {
        (self.include_type_only || !edge.is_type_only())
            && removed.is_none_or(|r| !r.contains(edge.from.idx()) && !r.contains(edge.to.idx()))
    }

    fn prune(&self, g: &Graph, ignore: &PathPatterns) -> Pruned {
        let mut removed = BitSet::new(g.module_count());
        let none = Captures::default();
        for m in g.module_ids() {
            if ignore.is_match(&g.module(m).path, &none) {
                removed.insert(m.idx());
            }
        }
        let scc = strongly_connected(g, &|e: &Edge| self.keeps(e, Some(&removed)));
        Pruned { scc, removed }
    }
}

impl Rule for NoCircular {
    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: NoCircularConfig = parse_options("no-circular", value)?;
        Ok(Self {
            include_type_only: c.include_type_only,
            ignore: (!c.ignore.is_empty())
                .then(|| PathPatterns::new(&c.ignore, Role::Referencing))
                .transpose()?,
            pruned: OnceLock::new(),
        })
    }

    fn requirements(&self) -> Analyses {
        // with `ignore`, the rule computes its own components
        match (&self.ignore, self.include_type_only) {
            (Some(_), _) => Analyses::empty(),
            (None, true) => Analyses::CYCLES,
            (None, false) => Analyses::CYCLES_RUNTIME,
        }
    }

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let g = cx.graph();
        let (scc, removed) = match &self.ignore {
            Some(ignore) => {
                let p = self.pruned.get_or_init(|| self.prune(g, ignore));
                (&p.scc, Some(&p.removed))
            }
            None => (cx.analyses().scc(!self.include_type_only), None),
        };
        let keep = |edge: &Edge| self.keeps(edge, removed);
        let Some(cycle) = shortest_cycle(g, scc, e, &keep) else {
            return;
        };
        let mut names: Vec<&str> = cycle
            .iter()
            .map(|s| cx.module(s.module).path.as_str())
            .collect();
        names.push(names[0]);
        cx.report_edge_with_path(
            e,
            format!("circular dependency: {}", names.join(" → ")),
            cycle,
        );
    }
}
