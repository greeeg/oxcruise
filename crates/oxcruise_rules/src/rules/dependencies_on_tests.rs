use oxcruise_core::{EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;

use super::{file_has_marker, in_test_dir};
use crate::meta::{Hooks, Rule, RuleCtx};

declare_rule! {
    /// ### What it does
    ///
    /// Reports a non-test module that imports a test file or a test directory.
    /// Test files match `*.test.*` or `*.spec.*`.
    /// Test directories are `test`, `tests`, `__tests__` and `__mocks__`.
    ///
    /// ### Why is this bad?
    ///
    /// Test code and its dependencies go into the production bundle, or crash there.
    DependenciesOnTests, category = correctness, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct DependenciesOnTests;
impl Rule for DependenciesOnTests {
    const HOOKS: Hooks = Hooks::EDGE;

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        let (from, to) = (cx.module(edge.from), cx.module(edge.to));
        if to.kind != ModuleKind::Local {
            return;
        }
        let is_test =
            |p: &str| file_has_marker(p, "test") || file_has_marker(p, "spec") || in_test_dir(p);
        if is_test(&to.path) && !is_test(&from.path) {
            cx.report_edge(
                e,
                format!("production code imports test code `{}`", to.path),
            );
        }
    }
}
