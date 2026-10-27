use oxcruise_core::{ModuleId, ModuleKind};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Rule, RuleCtx, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

/// Files that are legitimately unconnected: dotfiles, declarations, and tool configuration.
const DEFAULT_IGNORE: &[&str] = &["**/.*", "**/*.d.ts", "**/tsconfig*.json", "**/*.config.*"];

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NoOrphansConfig {
    /// Globs of modules that are never reported, on top of the defaults
    /// (`**/.*`, `**/*.d.ts`, `**/tsconfig*.json`, `**/*.config.*`).
    pub ignore: Vec<String>,
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
    /// `ignore`: globs for modules that are legitimately unconnected. They extend the built-in
    /// list: dotfiles (`**/.*`), declaration files (`**/*.d.ts`), `**/tsconfig*.json` and tool
    /// configuration (`**/*.config.*`).
    NoOrphans, category = suspicious, version = "0.1.0", config = NoOrphansConfig
}

#[derive(Debug)]
pub struct NoOrphans {
    ignore: PathPatterns,
}

impl Default for NoOrphans {
    fn default() -> Self {
        Self::build(&[]).expect("default ignore globs are valid")
    }
}

impl NoOrphans {
    fn build(extra: &[String]) -> Result<Self, String> {
        let globs: Vec<String> = DEFAULT_IGNORE
            .iter()
            .map(|g| g.to_string())
            .chain(extra.iter().cloned())
            .collect();
        Ok(Self {
            ignore: PathPatterns::new(&globs, Role::Referencing)?,
        })
    }
}

impl Rule for NoOrphans {
    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: NoOrphansConfig = parse_options("no-orphans", value)?;
        Self::build(&c.ignore)
    }

    fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        let g = cx.graph();
        let module = cx.module(m);
        if module.kind != ModuleKind::Source || g.in_degree(m) != 0 || g.out_degree(m) != 0 {
            return;
        }
        if self.ignore.is_match(&module.path, &Captures::default()) {
            return;
        }
        cx.report_module(
            m,
            "this module is not imported by any module and imports nothing",
        );
    }
}
