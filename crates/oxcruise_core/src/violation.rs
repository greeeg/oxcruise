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

/// An automatic repair: replace byte ranges of `file` (relative to the cruise root).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fix {
    pub file: String,
    /// `(start, end, replacement)`
    pub edits: Vec<(u32, u32, String)>,
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
    /// Index into `from`'s exports when the violation is about one export (for suppression).
    #[serde(default)]
    pub export: Option<u32>,
    /// File the violation is in when it is not a module (a `package.json`), relative to the root.
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub fix: Option<Fix>,
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
            export: None,
            file: None,
            fix: None,
        }
    }
}
