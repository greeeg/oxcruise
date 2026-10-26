//! The oxcruise rule engine.

pub mod declarative;
pub mod engine;
pub mod external;
pub mod meta;
pub mod pattern;
pub mod registry;
pub mod rules;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use engine::{Engine, RunResult};
pub use external::{
    ExternalRuleId, ExternalRuleSpec, ExternalRules, ExternalTarget, ExternalViolation, PluginRef,
};
pub use meta::{Analyses, Category, Rule, RuleCtx, RuleMeta};
pub use registry::{RuleEnum, RuleInfo, builtin_rules};
