use std::collections::HashMap;
use std::sync::OnceLock;

use oxcruise_core::{Edge, EdgeId, Graph, ModuleId};
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
    /// How many dependencies of one strongly connected component are searched for their shortest
    /// cycle (default 100); `0` removes the limit. Several of them can share one cycle.
    pub max_cycles_per_component: Option<u32>,
}

const DEFAULT_MAX_CYCLES_PER_COMPONENT: u32 = 100;

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
    ///
    /// `maxCyclesPerComponent`: a large tangle of modules has one cycle per dependency, and
    /// finding each costs a search of the whole tangle. Only this many dependencies per strongly
    /// connected component (the first ones in graph order, 100 by default) are searched; one
    /// extra violation on the component's first module says how many were left out. `0` searches
    /// them all.
    NoCircular, category = correctness, version = "0.1.0", config = NoCircularConfig
}

/// What the rule works out once per run: the components, which dependencies get a search, and
/// which components are cut short. Done up front so the result does not depend on how the
/// parallel per-edge checks are scheduled.
#[derive(Debug)]
struct Plan {
    /// Own components, only when `ignore` removes modules before the search.
    pruned: Option<(Scc, BitSet)>,
    /// Edges whose shortest cycle is searched (self-edges are always in).
    searched: BitSet,
    /// First module of each cut-short component -> (component size, dependencies left out).
    omitted: HashMap<ModuleId, (u32, u32)>,
}

#[derive(Debug, Default)]
pub struct NoCircular {
    include_type_only: bool,
    ignore: Option<PathPatterns>,
    max_per_component: u32,
    plan: OnceLock<Plan>,
}

impl NoCircular {
    fn keeps(&self, edge: &Edge, removed: Option<&BitSet>) -> bool {
        (self.include_type_only || !edge.is_type_only())
            && removed.is_none_or(|r| !r.contains(edge.from.idx()) && !r.contains(edge.to.idx()))
    }

    fn prune(&self, g: &Graph, ignore: &PathPatterns) -> (Scc, BitSet) {
        let mut removed = BitSet::new(g.module_count());
        let none = Captures::default();
        for m in g.module_ids() {
            if ignore.is_match(&g.module(m).path, &none) {
                removed.insert(m.idx());
            }
        }
        let scc = strongly_connected(g, &|e: &Edge| self.keeps(e, Some(&removed)));
        (scc, removed)
    }

    fn plan<'a>(&'a self, cx: &RuleCtx<'a>) -> &'a Plan {
        self.plan.get_or_init(|| {
            let g = cx.graph();
            let pruned = self.ignore.as_ref().map(|ignore| self.prune(g, ignore));
            let (scc, removed) = match &pruned {
                Some((scc, removed)) => (scc, Some(removed)),
                None => (cx.analyses().scc(!self.include_type_only), None),
            };
            let mut searched = BitSet::new(g.edge_count());
            let mut taken: HashMap<u32, u32> = HashMap::new();
            let mut left_out: HashMap<u32, u32> = HashMap::new();
            for id in (0..g.edge_count() as u32).map(EdgeId) {
                let e = g.edge(id);
                if !self.keeps(e, removed) || !scc.is_cyclic_edge(g, id) {
                    continue;
                }
                let n = taken.entry(scc.component(e.from)).or_default();
                if e.from == e.to || self.max_per_component == 0 || *n < self.max_per_component {
                    searched.insert(id.idx());
                    *n += 1;
                } else {
                    *left_out.entry(scc.component(e.from)).or_default() += 1;
                }
            }
            let mut omitted = HashMap::new();
            if !left_out.is_empty() {
                // module ids follow path order, so the first one met is the first by path
                for m in g.module_ids() {
                    if let Some(&n) = left_out.get(&scc.component(m)) {
                        omitted.entry(m).or_insert((scc.component_size(m), n));
                        left_out.remove(&scc.component(m));
                    }
                }
            }
            Plan {
                pruned,
                searched,
                omitted,
            }
        })
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
            max_per_component: c
                .max_cycles_per_component
                .unwrap_or(DEFAULT_MAX_CYCLES_PER_COMPONENT),
            plan: OnceLock::new(),
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

    fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        if let Some(&(size, n)) = self.plan(cx).omitted.get(&m) {
            cx.report_module(
                m,
                format!(
                    "circular dependencies: {n} more dependencies in this group of {size} \
                     modules were not searched for their cycle \
                     (maxCyclesPerComponent: {}); fix the cycles reported first, or raise the limit",
                    self.max_per_component
                ),
            );
        }
    }

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let g = cx.graph();
        let plan = self.plan(cx);
        if !plan.searched.contains(e.idx()) {
            return;
        }
        let (scc, removed) = match &plan.pruned {
            Some((scc, removed)) => (scc, Some(removed)),
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
