//! Rules that look only at the kinds of a single dependency.

use oxcruise_core::{DepKinds, EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Rule, RuleCtx, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

declare_rule! {
    /// ### What it does
    ///
    /// Reports imports of Node.js built-in modules that Node has deprecated, such as `sys` or
    /// `punycode`, and of npm packages whose `package.json` marks them as deprecated.
    NoDeprecated, category = suspicious, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct NoDeprecated;
impl Rule for NoDeprecated {
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
    /// The import only works because of hoisting or a transitive install, and
    /// breaks when the dependency tree changes.
    NoUndeclaredDeps, category = correctness, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct NoUndeclaredDeps;
impl Rule for NoUndeclaredDeps {
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
pub struct NoUnresolvableConfig {
    /// Globs of imports that are allowed to be unresolvable. An import is skipped when its
    /// specifier (`virtual:icons`) or the importing module's path matches.
    pub ignore: Vec<String>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports imports that could not be resolved to a file or package.
    ///
    /// ### Options
    ///
    /// `ignore`: globs matched against the import specifier and the importing module's path,
    /// for virtual modules and generated code.
    NoUnresolvable, category = correctness, version = "0.1.0", config = NoUnresolvableConfig
}
#[derive(Debug, Default)]
pub struct NoUnresolvable {
    ignore: Option<PathPatterns>,
}
impl Rule for NoUnresolvable {
    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: NoUnresolvableConfig = parse_options("no-unresolvable", value)?;
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
    /// Reports packages declared in more than one `package.json` section, for example both
    /// `dependencies` and `devDependencies`. Type-only imports are ignored.
    NoDuplicateDeps, category = suspicious, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct NoDuplicateDeps;
impl Rule for NoDuplicateDeps {
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
    NoOptionalDeps, category = restriction, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct NoOptionalDeps;
impl Rule for NoOptionalDeps {
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
    NoPeerDeps, category = restriction, version = "0.1.0"
}
#[derive(Debug, Default)]
pub struct NoPeerDeps;
impl Rule for NoPeerDeps {
    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        if edge.kinds.contains(DepKinds::NPM_PEER) {
            cx.report_edge(e, format!("`{}` is a peer dependency", edge.specifier));
        }
    }
}
