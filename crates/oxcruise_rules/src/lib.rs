//! The oxcruise rule engine.

pub mod conventions;
pub mod declarative;
pub mod engine;
pub mod entries;
pub mod external;
pub mod meta;
pub mod pattern;
pub mod registry;
pub mod rules;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use conventions::ConventionFilter;
pub use engine::{Engine, RunResult};
pub use external::{
    ExternalRuleId, ExternalRuleSpec, ExternalRules, ExternalTarget, ExternalViolation, PluginRef,
};
pub use meta::{Analyses, AnalysisCache, Category, Hooks, Rule, RuleCtx, RuleMeta};
pub use registry::{RuleEnum, RuleInfo, builtin_rules};
