use oxcruise_core::{EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;

use super::{file_has_marker, in_test_dir};
use crate::meta::{Rule, RuleCtx};

declare_rule! {
    /// ### What it does
    ///
    /// Reports non-test modules importing test files (`*.test.*`, `*.spec.*`) or test
    /// directories (`test`, `tests`, `__tests__`, `__mocks__`).
    ///
    /// ### Why is this bad?
    ///
    /// Test code and its dependencies end up in the production bundle, or crash there.
    NoTestImports, category = correctness, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct NoTestImports;
impl Rule for NoTestImports {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        let (from, to) = (cx.module(edge.from), cx.module(edge.to));
        if to.kind != ModuleKind::Source {
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
