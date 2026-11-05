//! Callback interface to out-of-process (JS) rules. No napi dependency lives here:
//! the `oxcruise` app implements [`ExternalRules`] on top of its napi callbacks.

use oxcruise_core::{EdgeId, Graph, ModuleId, ViolationLevel};
use oxcruise_graph::analyses::ExportUsage;
use oxcruise_resolve::WorkspacePackage;

use crate::meta::AnalysisCache;

/// Index of an external rule within one run (`offset + i` in oxlint's scheme).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExternalRuleId(pub u32);

#[derive(Debug, Clone)]
pub struct ExternalRuleSpec {
    pub id: ExternalRuleId,
    pub name: String,
    pub level: ViolationLevel,
    pub options: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalTarget {
    Module(ModuleId),
    Edge(EdgeId),
    /// Export number `.1` of the module (in `Module::exports`).
    Export(ModuleId, u32),
}

#[derive(Debug, Clone)]
pub struct ExternalViolation {
    pub rule: ExternalRuleId,
    pub target: ExternalTarget,
    pub message: String,
}

/// One entry of `jsPlugins`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRef {
    /// A path (relative to the cwd) or a package name.
    pub specifier: String,
    /// Register the plugin's rules under this name instead of its own.
    pub alias: Option<String>,
}

pub trait ExternalRules: Send + Sync {
    /// Load the plugins named in `jsPlugins`. Called once, before any rule is looked up.
    fn load_plugins(&self, _plugins: &[PluginRef], _cwd: &std::path::Path) -> Result<(), String> {
        Ok(())
    }

    /// Globs of modules the loaded plugins declare as entry points (`definePlugin({ entries })`),
    /// relative to the cruise root.
    fn entries(&self) -> Vec<String> {
        vec![]
    }

    /// Whether a loaded plugin reads exports and used names (`definePlugin({ needs: ["symbols"] })`).
    fn needs_symbols(&self) -> bool {
        false
    }

    /// Is there a plugin rule with this fully qualified name (`acme/max-fan-out`)?
    fn rule_exists(&self, name: &str) -> bool;

    /// Run every given rule over the finished graph in **one** call.
    fn run(
        &self,
        graph: &Graph,
        workspace: &[WorkspacePackage],
        rules: &[ExternalRuleSpec],
        // which exports are used, when symbols were extracted
        usage: Option<&ExportUsage>,
        // analyses already computed for the built-in rules (`CYCLES` and `DEPENDENTS` are always
        // there when plugin rules run), so the host need not repeat them
        analyses: &AnalysisCache,
    ) -> Result<Vec<ExternalViolation>, String>;
}
