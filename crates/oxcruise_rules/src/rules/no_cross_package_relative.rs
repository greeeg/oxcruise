use oxcruise_core::{EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;

use crate::meta::{Rule, RuleCtx};

declare_rule! {
    /// ### What it does
    ///
    /// Reports relative imports (`../other-package/src/x`) that reach into another workspace package.
    ///
    /// ### Why is this bad?
    ///
    /// Packages should depend on each other through their package name so the dependency is
    /// declared in `package.json` and the package boundary is respected.
    NoCrossPackageRelative, category = correctness, version = "0.1.0"
}

#[derive(Debug, Default)]
pub struct NoCrossPackageRelative;
impl Rule for NoCrossPackageRelative {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if !(edge.specifier.starts_with("./") || edge.specifier.starts_with("../")) {
            return;
        }
        let (from, to) = (cx.module(edge.from), cx.module(edge.to));
        if to.kind != ModuleKind::Source {
            return;
        }
        if let (Some(a), Some(b)) = (from.package, to.package)
            && a != b
        {
            let name = |i: u32| {
                cx.shared
                    .workspace
                    .get(i as usize)
                    .map_or("?", |p| p.name.as_str())
            };
            cx.report_edge(
                e,
                format!("relative import crosses from package `{}` into `{}`; import it by package name", name(a), name(b)),
            );
        }
    }
}
