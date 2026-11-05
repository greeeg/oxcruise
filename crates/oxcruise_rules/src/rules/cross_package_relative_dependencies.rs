use oxcruise_core::{EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;

use crate::meta::{Hooks, Rule, RuleCtx};

declare_rule! {
    /// ### What it does
    ///
    /// Reports relative imports that reach into another workspace package, for example `../other-package/src/x`.
    ///
    /// ### Why is this bad?
    ///
    /// A package must depend on another package through the package name.
    /// Then `package.json` declares the dependency, and the package boundary stays intact.
    CrossPackageRelativeDependencies, category = correctness, version = "0.1.0"
}

#[derive(Debug, Default)]
pub struct CrossPackageRelativeDependencies;
impl Rule for CrossPackageRelativeDependencies {
    const HOOKS: Hooks = Hooks::EDGE;

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if !(edge.specifier.starts_with("./") || edge.specifier.starts_with("../")) {
            return;
        }
        let (from, to) = (cx.module(edge.from), cx.module(edge.to));
        if to.kind != ModuleKind::Local {
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
