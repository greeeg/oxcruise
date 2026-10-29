use serde::{Deserialize, Serialize};

use crate::{EdgeId, Loc, ModuleId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warn,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
        }
    }
}

/// One hop of a cycle or reachability path. `edge` is the edge that leaves
/// `module` towards the next step (`None` on the last step).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CycleStep {
    pub module: ModuleId,
    pub edge: Option<EdgeId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    pub rule: String,
    pub severity: Severity,
    pub from: ModuleId,
    pub to: Option<ModuleId>,
    /// Edge the violation is attached to, if any (used for line/col and
    /// suppression comments).
    pub edge: Option<EdgeId>,
    pub message: String,
    pub loc: Option<Loc>,
    /// Cycle or path that explains the violation.
    pub path: Vec<CycleStep>,
}

impl Violation {
    pub fn module(
        rule: impl Into<String>,
        severity: Severity,
        from: ModuleId,
        message: String,
    ) -> Self {
        Self {
            rule: rule.into(),
            severity,
            from,
            to: None,
            edge: None,
            message,
            loc: None,
            path: vec![],
        }
    }
}
