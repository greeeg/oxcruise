use oxcruise_core::{DepKinds, EdgeId};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use super::is_test_module;
use crate::meta::{Hooks, Rule, RuleCtx, RuleMeta, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct DevDependenciesInProductionConfig {
    /// Globs of production code. Defaults to `src`, `app`, `lib` and `packages/*/src`.
    pub prod: Vec<String>,
    /// Globs of test code, exempt from the rule (on top of `*.test.*`, `*.spec.*` and test directories).
    pub test: Vec<String>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports a production module that imports a package from `devDependencies` only.
    ///
    /// ### Why is this bad?
    ///
    /// A production install does not include dev dependencies, so the import fails there.
    /// The rule ignores type-only imports, because they do not exist at runtime.
    ///
    /// ### Options
    ///
    /// * `prod`: globs of production code. The default is `src`, `app`, `lib` and `packages/*/src`.
    /// * `test`: globs of test code. The rule does not apply to test code.
    ///   These globs add to `*.test.*`, `*.spec.*` and test directories.
    DevDependenciesInProduction, category = correctness, version = "0.1.0", config = DevDependenciesInProductionConfig
}

#[derive(Debug, Default)]
pub struct DevDependenciesInProduction {
    prod: Option<PathPatterns>,
    test: Option<PathPatterns>,
}

impl Rule for DevDependenciesInProduction {
    const HOOKS: Hooks = Hooks::EDGE;

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let mut c: DevDependenciesInProductionConfig = parse_options(Self::NAME, value)?;
        if c.prod.is_empty() {
            c.prod = ["src", "app", "lib", "packages/*/src"]
                .map(String::from)
                .into();
        }
        Ok(Self {
            prod: Some(PathPatterns::new(&c.prod, Role::Referencing)?),
            test: Some(PathPatterns::new(&c.test, Role::Referencing)?),
        })
    }

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if !edge.kinds.contains(DepKinds::NPM_DEV)
            || edge.kinds.contains(DepKinds::NPM)
            || edge.is_type_only()
        {
            return;
        }
        let path = &cx.module(edge.from).path;
        let none = Captures::default();
        let is_test = self.test.as_ref().map_or_else(
            || is_test_module(path, &PathPatterns::empty()),
            |t| is_test_module(path, t),
        );
        let is_prod = self.prod.as_ref().is_none_or(|p| p.is_match(path, &none));
        if is_prod && !is_test {
            cx.report_edge(
                e,
                format!(
                    "production code imports dev dependency `{}`",
                    edge.specifier
                ),
            );
        }
    }
}
