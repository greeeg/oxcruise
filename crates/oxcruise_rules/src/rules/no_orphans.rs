use oxcruise_config::PathSpec;
use oxcruise_core::{ModuleId, ModuleKind};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Rule, RuleCtx, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NoOrphansConfig {
    /// Modules matching these patterns are never reported (config files, `.d.ts`, ...).
    pub ignore: Vec<PathSpec>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports modules that no other module imports and that import nothing.
    ///
    /// ### Why is this bad?
    ///
    /// An orphan is usually dead code left behind by a refactor.
    ///
    /// ### Options
    ///
    /// `ignore`: patterns for modules that are legitimately unconnected, such as
    /// configuration files or type declarations.
    NoOrphans, category = suspicious, version = "0.1.0", config = NoOrphansConfig, recommended
}

#[derive(Debug, Default)]
pub struct NoOrphans {
    ignore: Option<PathPatterns>,
}

impl Rule for NoOrphans {
    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: NoOrphansConfig = parse_options("no-orphans", value)?;
        Ok(Self {
            ignore: Some(PathPatterns::new(&c.ignore, Role::Referencing)?),
        })
    }

    fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        let g = cx.graph();
        let module = cx.module(m);
        if module.kind != ModuleKind::Source || g.in_degree(m) != 0 || g.out_degree(m) != 0 {
            return;
        }
        if self
            .ignore
            .as_ref()
            .is_some_and(|p| p.is_match(&module.path, &Captures::default()))
        {
            return;
        }
        cx.report_module(
            m,
            "this module is not imported by any module and imports nothing",
        );
    }
}
