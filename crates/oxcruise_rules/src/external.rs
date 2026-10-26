//! Callback interface to out-of-process (JS) rules. No napi dependency lives here:
//! the `oxcruise` app implements [`ExternalRules`] on top of its napi callbacks.

use oxcruise_core::{EdgeId, Graph, ModuleId, Severity};
use oxcruise_resolve::WorkspacePackage;

/// Index of an external rule within one run (`offset + i` in oxlint's scheme).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExternalRuleId(pub u32);

#[derive(Debug, Clone)]
pub struct ExternalRuleSpec {
    pub id: ExternalRuleId,
    pub name: String,
    pub severity: Severity,
    pub options: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalTarget {
    Module(ModuleId),
    Edge(EdgeId),
}

#[derive(Debug, Clone)]
pub struct ExternalViolation {
    pub rule: ExternalRuleId,
    pub target: ExternalTarget,
    pub message: String,
}

pub trait ExternalRules: Send + Sync {
    /// Is there a plugin rule with this fully qualified name (`acme/max-fan-out`)?
    fn rule_exists(&self, name: &str) -> bool;

    /// Run every given rule over the finished graph in **one** call.
    fn run(
        &self,
        graph: &Graph,
        workspace: &[WorkspacePackage],
        rules: &[ExternalRuleSpec],
    ) -> Result<Vec<ExternalViolation>, String>;
}
