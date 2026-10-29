use std::sync::Arc;

use oxcruise_core::{EdgeId, ModuleId};

use crate::declarative::CompiledRule;
use crate::meta::{Analyses, Category, Rule, RuleCtx, RuleMeta};
use crate::rules::*;

/// Static description of a rule, for `--rules` and docs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleInfo {
    pub name: &'static str,
    pub category: Category,
    pub version: &'static str,
    pub docs: &'static str,
}

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

            pub fn requirements(&self) -> Analyses {
                match self {
                    $(RuleEnum::$ty(r) => r.requirements(),)*
                    RuleEnum::Declarative(r) => r.requirements(),
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
                    RuleEnum::Declarative(r) => r.check_edge(e, cx),
                }
            }

            pub fn check_graph(&self, cx: &RuleCtx<'_>) {
                match self {
                    $(RuleEnum::$ty(r) => r.check_graph(cx),)*
                    RuleEnum::Declarative(_) => {}
                }
            }
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
    NoCircular,
    NoOrphans,
    NoDeprecated,
    NoUndeclaredDeps,
    NoUnresolvable,
    NoDuplicateDeps,
    NoTestImports,
    NoDevDepsInProd,
    NoOptionalDeps,
    NoPeerDeps,
    Layers,
    InternalModules,
    NoCrossPackageRelative,
}
