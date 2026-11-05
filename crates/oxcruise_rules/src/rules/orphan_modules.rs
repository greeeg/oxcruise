use oxcruise_core::{ModuleId, ModuleKind};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Hooks, Rule, RuleCtx, RuleMeta, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

/// Files that are legitimately unconnected: dotfiles, declarations, and tool configuration.
const DEFAULT_IGNORE: &[&str] = &["**/.*", "**/*.d.ts", "**/tsconfig*.json", "**/*.config.*"];

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct OrphanModulesConfig {
    /// Globs of modules that are never reported, on top of the defaults
    /// (`**/.*`, `**/*.d.ts`, `**/tsconfig*.json`, `**/*.config.*`).
    pub ignore: Vec<String>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports modules that no other module imports and that import no module.
    ///
    /// ### Why is this bad?
    ///
    /// An orphan is usually dead code that a refactor left behind.
    ///
    /// ### Options
    ///
    /// * `ignore`: globs of modules that have no connection on purpose.
    ///   These globs add to the built-in list: dotfiles (`**/.*`), declaration files (`**/*.d.ts`),
    ///   `**/tsconfig*.json` and the configuration files of tools (`**/*.config.*`).
    OrphanModules, category = suspicious, version = "0.1.0", config = OrphanModulesConfig
}

#[derive(Debug)]
pub struct OrphanModules {
    ignore: PathPatterns,
}

impl Default for OrphanModules {
    fn default() -> Self {
        Self::build(&[]).expect("default ignore globs are valid")
    }
}

impl OrphanModules {
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

impl Rule for OrphanModules {
    const HOOKS: Hooks = Hooks::MODULE;

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: OrphanModulesConfig = parse_options(Self::NAME, value)?;
        Self::build(&c.ignore)
    }

    fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        let g = cx.graph();
        let module = cx.module(m);
        if module.kind != ModuleKind::Local || g.in_degree(m) != 0 || g.out_degree(m) != 0 {
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
