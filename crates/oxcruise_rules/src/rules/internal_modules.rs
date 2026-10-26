use oxcruise_core::{EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Rule, RuleCtx, parse_options};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct InternalModulesConfig {
    /// Directory name that marks code as internal. Defaults to `internal`.
    pub marker: String,
}

impl Default for InternalModulesConfig {
    fn default() -> Self {
        Self {
            marker: "internal".into(),
        }
    }
}

declare_rule! {
    /// ### What it does
    ///
    /// Go-style `internal` directories: code under `a/b/internal/` may only be imported from code under `a/b/`.
    InternalModules, category = restriction, version = "0.1.0", config = InternalModulesConfig
}

#[derive(Debug)]
pub struct InternalModules {
    marker: String,
}

impl Default for InternalModules {
    fn default() -> Self {
        Self {
            marker: InternalModulesConfig::default().marker,
        }
    }
}

impl Rule for InternalModules {
    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: InternalModulesConfig = parse_options("internal-modules", value)?;
        Ok(Self { marker: c.marker })
    }

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        let (from, to) = (cx.module(edge.from), cx.module(edge.to));
        if to.kind != ModuleKind::Source {
            return;
        }
        let segs: Vec<&str> = to.path.split('/').collect();
        // Only directory segments count, never the file name.
        let Some(i) = segs[..segs.len() - 1]
            .iter()
            .position(|s| *s == self.marker)
        else {
            return;
        };
        let owner = segs[..i].join("/");
        let allowed =
            owner.is_empty() || from.path == owner || from.path.starts_with(&format!("{owner}/"));
        if !allowed {
            cx.report_edge(
                e,
                format!(
                    "`{}` is internal to `{owner}` and cannot be imported from `{}`",
                    to.path, from.path
                ),
            );
        }
    }
}
