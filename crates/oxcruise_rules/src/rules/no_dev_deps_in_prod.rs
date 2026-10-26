use oxcruise_config::PathSpec;
use oxcruise_core::{DepKinds, EdgeId};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use super::{file_has_marker, in_test_dir};
use crate::meta::{Rule, RuleCtx, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NoDevDepsInProdConfig {
    /// Globs of production code. Defaults to `src`, `app`, `lib` and `packages/*/src`.
    pub prod: Vec<PathSpec>,
    /// Globs of test code, exempt from the rule (on top of `*.test.*`, `*.spec.*` and test directories).
    pub test: Vec<PathSpec>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports production modules importing packages that are only in `devDependencies`.
    ///
    /// ### Why is this bad?
    ///
    /// Dev dependencies are not installed in production, so the import fails there.
    /// Type-only imports are fine and are ignored.
    ///
    /// ### Options
    ///
    /// `prod` and `test` are path patterns that say which modules are production and which are tests.
    NoDevDepsInProd, category = correctness, version = "0.1.0", config = NoDevDepsInProdConfig, recommended
}

#[derive(Debug, Default)]
pub struct NoDevDepsInProd {
    prod: Option<PathPatterns>,
    test: Option<PathPatterns>,
}

impl Rule for NoDevDepsInProd {
    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let mut c: NoDevDepsInProdConfig = parse_options("no-dev-deps-in-prod", value)?;
        if c.prod.is_empty() {
            c.prod = ["src", "app", "lib", "packages/*/src"]
                .map(PathSpec::glob)
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
        let is_test = file_has_marker(path, "test")
            || file_has_marker(path, "spec")
            || in_test_dir(path)
            || self.test.as_ref().is_some_and(|t| t.is_match(path, &none));
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
