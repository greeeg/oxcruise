use oxcruise_core::{EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;

use super::{file_has_marker, in_test_dir};
use crate::meta::{Rule, RuleCtx};

declare_rule! {
    /// ### What it does
    ///
    /// Reports non-test modules importing test files (`*.test.ts`) or test directories.
    NotToTest, category = correctness, version = "0.1.0", recommended
}
#[derive(Debug, Default)]
pub struct NotToTest;
impl Rule for NotToTest {
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

declare_rule! {
    /// ### What it does
    ///
    /// Reports non-spec modules importing `*.spec.*` files.
    NotToSpec, category = correctness, version = "0.1.0", recommended
}
#[derive(Debug, Default)]
pub struct NotToSpec;
impl Rule for NotToSpec {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        let (from, to) = (cx.module(edge.from), cx.module(edge.to));
        if to.kind == ModuleKind::Source
            && file_has_marker(&to.path, "spec")
            && !file_has_marker(&from.path, "spec")
        {
            cx.report_edge(
                e,
                format!("production code imports spec file `{}`", to.path),
            );
        }
    }
}
