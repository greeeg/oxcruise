use std::sync::Arc;

use oxcruise_core::{EdgeId, ModuleId};

use crate::conventions::ConventionFilter;
use crate::declarative::CompiledRule;
use crate::meta::{Analyses, Category, Hooks, Rule, RuleCtx, RuleMeta};
use crate::rules::*;

/// Static description of a rule, for `--rules` and docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleInfo {
    pub name: &'static str,
    pub category: Category,
    pub version: &'static str,
    pub docs: &'static str,
}

/// Built-in rules that have a new name: `(old, new)`. The unknown-rule error uses it.
pub const RENAMED: &[(&str, &str)] = &[
    ("no-circular", "circular-dependencies"),
    ("no-orphans", "orphan-modules"),
    ("no-deprecated", "deprecated-dependencies"),
    ("no-undeclared-deps", "undeclared-packages"),
    ("no-unresolvable", "unresolved-dependencies"),
    ("no-duplicate-deps", "duplicate-packages"),
    ("no-optional-deps", "optional-dependencies"),
    ("no-peer-deps", "peer-dependencies"),
    ("no-test-imports", "dependencies-on-tests"),
    ("no-dev-deps-in-prod", "dev-dependencies-in-production"),
    (
        "no-cross-package-relative",
        "cross-package-relative-dependencies",
    ),
    ("no-unused-exports", "unused-exports"),
    ("no-unused-deps", "unused-packages"),
    ("no-unused-files", "unused-modules"),
    ("internal-modules", "visibility"),
];

macro_rules! register_rules {
    ($($ty:ident),* $(,)?) => {
        /// Every rule the engine can run: built-ins plus compiled declarative rules.
        #[derive(Debug)]
        pub enum RuleEnum {
            $($ty($ty),)*
            Declarative(Arc<CompiledRule>),
        }

        impl RuleEnum {
            /// Instantiate a built-in rule by name. `None` if there is no such rule.
            pub fn builtin(name: &str, options: serde_json::Value) -> Option<Result<RuleEnum, String>> {
                $(if name == <$ty as RuleMeta>::NAME {
                    return Some(<$ty as Rule>::from_configuration(options).map(RuleEnum::$ty));
                })*
                None
            }

            /// Whether the built-in rule `name` has a `production` option (`--production`).
            pub fn has_production_option(name: &str) -> bool {
                $(if name == <$ty as RuleMeta>::NAME {
                    return <$ty as Rule>::has_production_option();
                })*
                false
            }

            pub fn requirements(&self) -> Analyses {
                match self {
                    $(RuleEnum::$ty(r) => r.requirements(),)*
                    RuleEnum::Declarative(r) => r.requirements(),
                }
            }

            pub fn conventions(&self) -> ConventionFilter {
                match self {
                    $(RuleEnum::$ty(r) => r.conventions(),)*
                    RuleEnum::Declarative(_) => ConventionFilter::default(),
                }
            }

            /// The hooks this rule implements.
            pub fn hooks(&self) -> Hooks {
                match self {
                    $(RuleEnum::$ty(_) => <$ty as Rule>::HOOKS,)*
                    RuleEnum::Declarative(r) => r.hooks(),
                }
            }

            pub fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
                match self {
                    $(RuleEnum::$ty(r) => r.check_module(m, cx),)*
                    RuleEnum::Declarative(r) => r.check_module(m, cx),
                }
            }

            pub fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
                match self {
                    $(RuleEnum::$ty(r) => r.check_edge(e, cx),)*
                    RuleEnum::Declarative(_) => {}
                }
            }

            pub fn check_graph(&self, cx: &RuleCtx<'_>) {
                match self {
                    $(RuleEnum::$ty(r) => r.check_graph(cx),)*
                    RuleEnum::Declarative(_) => {}
                }
            }
        }

        /// The JSON schema of the options of every built-in rule, sorted by name. `None` for a rule
        /// without options.
        #[cfg(feature = "ruledocs")]
        pub fn builtin_option_schemas() -> Vec<(&'static str, Option<serde_json::Value>)> {
            let mut v = vec![$((<$ty as RuleMeta>::NAME, <$ty as RuleMeta>::config_schema()),)*];
            v.sort_by_key(|(name, _)| *name);
            v
        }

        /// All built-in rules, sorted by name.
        pub fn builtin_rules() -> Vec<RuleInfo> {
            let mut v = vec![$(RuleInfo {
                name: <$ty as RuleMeta>::NAME,
                category: <$ty as RuleMeta>::CATEGORY,
                version: <$ty as RuleMeta>::VERSION,
                docs: <$ty as RuleMeta>::DOCS,
            },)*];
            v.sort_by_key(|r| r.name);
            v
        }
    };
}

register_rules! {
    CircularDependencies,
    OrphanModules,
    DeprecatedDependencies,
    UndeclaredPackages,
    UnresolvedDependencies,
    DuplicatePackages,
    DependenciesOnTests,
    DevDependenciesInProduction,
    OptionalDependencies,
    PeerDependencies,
    Layers,
    Visibility,
    CrossPackageRelativeDependencies,
    UnusedExports,
    UnusedPackages,
    UnusedModules,
}
