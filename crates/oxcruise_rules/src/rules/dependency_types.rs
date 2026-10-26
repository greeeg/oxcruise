//! Rules that look only at the kinds of a single dependency.

use oxcruise_core::{DepKinds, EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;

use crate::meta::{Rule, RuleCtx};

declare_rule! {
    /// ### What it does
    ///
    /// Reports imports of Node.js built-in modules that Node has deprecated, such as `sys` or `punycode`.
    NoDeprecatedCore, category = correctness, version = "0.1.0", recommended
}
#[derive(Debug, Default)]
pub struct NoDeprecatedCore;
impl Rule for NoDeprecatedCore {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if edge.kinds.contains(DepKinds::CORE | DepKinds::DEPRECATED) {
            cx.report_edge(
                e,
                format!("`{}` is a deprecated Node.js module", edge.specifier),
            );
        }
    }
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports imports of npm packages whose `package.json` marks them as deprecated.
    NoDeprecatedNpm, category = suspicious, version = "0.1.0", recommended
}
#[derive(Debug, Default)]
pub struct NoDeprecatedNpm;
impl Rule for NoDeprecatedNpm {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if edge.kinds.contains(DepKinds::DEPRECATED) && edge.kinds.intersects(DepKinds::NPM_ANY) {
            cx.report_edge(
                e,
                format!("`{}` is deprecated by its maintainers", edge.specifier),
            );
        }
    }
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports imports of installed npm packages that no `package.json` section declares.
    ///
    /// ### Why is this bad?
    ///
    /// The import only works because of hoisting or a transitive install, and
    /// breaks when the dependency tree changes.
    NoNonPackageJson, category = correctness, version = "0.1.0", recommended
}
#[derive(Debug, Default)]
pub struct NoNonPackageJson;
impl Rule for NoNonPackageJson {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if edge
            .kinds
            .intersects(DepKinds::NPM_NO_PKG | DepKinds::NPM_UNKNOWN)
        {
            cx.report_edge(
                e,
                format!("`{}` is not declared in package.json", edge.specifier),
            );
        }
    }
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports imports that could not be resolved to a file or package.
    NoUnresolvable, category = correctness, version = "0.1.0", recommended
}
#[derive(Debug, Default)]
pub struct NoUnresolvable;
impl Rule for NoUnresolvable {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if cx.module(edge.to).kind == ModuleKind::Unresolved {
            cx.report_edge(e, format!("cannot resolve `{}`", edge.specifier));
        }
    }
}

const NPM_SECTIONS: DepKinds = DepKinds::NPM
    .union(DepKinds::NPM_DEV)
    .union(DepKinds::NPM_PEER)
    .union(DepKinds::NPM_OPTIONAL)
    .union(DepKinds::NPM_BUNDLED);

declare_rule! {
    /// ### What it does
    ///
    /// Reports packages declared in more than one `package.json` section, for example both
    /// `dependencies` and `devDependencies`. Type-only imports are ignored.
    NoDuplicateDepTypes, category = suspicious, version = "0.1.0", recommended
}
#[derive(Debug, Default)]
pub struct NoDuplicateDepTypes;
impl Rule for NoDuplicateDepTypes {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if !edge.is_type_only() && (edge.kinds & NPM_SECTIONS).bits().count_ones() > 1 {
            let names: Vec<_> = (edge.kinds & NPM_SECTIONS).names().collect();
            cx.report_edge(
                e,
                format!(
                    "`{}` is declared as {}",
                    edge.specifier,
                    names.join(" and ")
                ),
            );
        }
    }
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports use of optional dependencies, which may not be installed.
    OptionalDepsUsed, category = restriction, version = "0.1.0", recommended
}
#[derive(Debug, Default)]
pub struct OptionalDepsUsed;
impl Rule for OptionalDepsUsed {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if edge.kinds.contains(DepKinds::NPM_OPTIONAL) {
            cx.report_edge(
                e,
                format!(
                    "`{}` is an optional dependency and may not be installed",
                    edge.specifier
                ),
            );
        }
    }
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports use of peer dependencies, which the consuming project must provide.
    PeerDepsUsed, category = restriction, version = "0.1.0", recommended
}
#[derive(Debug, Default)]
pub struct PeerDepsUsed;
impl Rule for PeerDepsUsed {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if edge.kinds.contains(DepKinds::NPM_PEER) {
            cx.report_edge(e, format!("`{}` is a peer dependency", edge.specifier));
        }
    }
}
