//! Rules that look only at the kinds of a single dependency.

use oxcruise_core::{DepKinds, EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Hooks, Rule, RuleCtx, RuleMeta, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

declare_rule! {
    /// ### What it does
    ///
    /// Reports imports of deprecated Node.js built-in modules and deprecated npm packages.
    ///
    /// Node.js deprecates built-in modules such as `sys` and `punycode`.
    /// An npm package has a deprecation mark in its own `package.json`.
    DeprecatedDependencies, category = suspicious, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct DeprecatedDependencies;
impl Rule for DeprecatedDependencies {
    const HOOKS: Hooks = Hooks::EDGE;

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if !edge.kinds.contains(DepKinds::DEPRECATED) {
            return;
        }
        if edge.kinds.contains(DepKinds::CORE) {
            cx.report_edge(
                e,
                format!("`{}` is a deprecated Node.js module", edge.specifier),
            );
        } else if edge.kinds.intersects(DepKinds::NPM_ANY) {
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
    /// The import works only because of hoisting or a transitive install.
    /// It breaks when the dependency tree changes.
    UndeclaredPackages, category = correctness, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct UndeclaredPackages;
impl Rule for UndeclaredPackages {
    const HOOKS: Hooks = Hooks::EDGE;

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

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct UnresolvedDependenciesConfig {
    /// Globs of imports that are allowed to be unresolvable. An import is skipped when its
    /// specifier (`virtual:icons`) or the importing module's path matches.
    pub ignore: Vec<String>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports an import that oxcruise cannot resolve to a file or a package.
    ///
    /// ### Options
    ///
    /// * `ignore`: globs for virtual modules and generated code.
    ///   The rule matches each glob against the import specifier and against the path of the importing module.
    UnresolvedDependencies, category = correctness, version = "0.1.0", config = UnresolvedDependenciesConfig
}
#[derive(Debug, Default)]
pub struct UnresolvedDependencies {
    ignore: Option<PathPatterns>,
}
impl Rule for UnresolvedDependencies {
    const HOOKS: Hooks = Hooks::EDGE;

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: UnresolvedDependenciesConfig = parse_options(Self::NAME, value)?;
        Ok(Self {
            ignore: Some(PathPatterns::new(&c.ignore, Role::Referencing)?),
        })
    }

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if cx.module(edge.to).kind != ModuleKind::Unresolved {
            return;
        }
        if let Some(ignore) = &self.ignore {
            let none = Captures::default();
            if ignore.is_match(&edge.specifier, &none)
                || ignore.is_match(&cx.module(edge.from).path, &none)
            {
                return;
            }
        }
        cx.report_edge(e, format!("cannot resolve `{}`", edge.specifier));
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
    /// Reports a package that more than one `package.json` section declares,
    /// for example both `dependencies` and `devDependencies`.
    /// The rule ignores type-only imports.
    DuplicatePackages, category = suspicious, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct DuplicatePackages;
impl Rule for DuplicatePackages {
    const HOOKS: Hooks = Hooks::EDGE;

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
    /// Reports use of optional dependencies.
    /// An install can skip an optional dependency, so the package may not exist at runtime.
    OptionalDependencies, category = restriction, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct OptionalDependencies;
impl Rule for OptionalDependencies {
    const HOOKS: Hooks = Hooks::EDGE;

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
    /// Reports use of peer dependencies.
    /// The consuming project must provide a peer dependency.
    PeerDependencies, category = restriction, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct PeerDependencies;
impl Rule for PeerDependencies {
    const HOOKS: Hooks = Hooks::EDGE;

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if edge.kinds.contains(DepKinds::NPM_PEER) {
            cx.report_edge(e, format!("`{}` is a peer dependency", edge.specifier));
        }
    }
}
