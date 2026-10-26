use oxcruise_core::{Edge, EdgeId};
use oxcruise_graph::analyses::shortest_cycle;
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Analyses, Rule, RuleCtx, parse_options};

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NoCircularConfig {
    /// Ignore type-only imports, so only cycles that exist at runtime are reported.
    pub ignore_type_only: bool,
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
    /// `ignoreTypeOnly`: drop type-only imports *before* looking for cycles, so
    /// a cycle that exists only through `import type` is not reported.
    NoCircular, category = correctness, version = "0.1.0", config = NoCircularConfig, recommended
}

#[derive(Debug, Default)]
pub struct NoCircular {
    ignore_type_only: bool,
}

impl Rule for NoCircular {
    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: NoCircularConfig = parse_options("no-circular", value)?;
        Ok(Self {
            ignore_type_only: c.ignore_type_only,
        })
    }

    fn requirements(&self) -> Analyses {
        if self.ignore_type_only {
            Analyses::CYCLES_RUNTIME
        } else {
            Analyses::CYCLES
        }
    }

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let g = cx.graph();
        let scc = cx.analyses().scc(self.ignore_type_only);
        let ignore = self.ignore_type_only;
        let keep = move |edge: &Edge| !(ignore && edge.is_type_only());
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
