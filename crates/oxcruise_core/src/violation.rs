use serde::{Deserialize, Serialize};

use crate::{EdgeId, Loc, ModuleId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViolationLevel {
    Error,
    Warn,
}

impl ViolationLevel {
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
    pub level: ViolationLevel,
    pub from: ModuleId,
    pub to: Option<ModuleId>,
    /// Edge the violation is attached to, if any (used for line/col and
    /// suppression comments).
    pub edge: Option<EdgeId>,
    pub message: String,
    pub loc: Option<Loc>,
    /// Cycle or path that explains the violation.
    pub path: Vec<CycleStep>,
    /// What the violation is about, besides `from`.
    #[serde(default)]
    pub subject: Subject,
    #[serde(default)]
    pub fix: Option<Fix>,
}

/// What a violation is attached to, which decides how comments can suppress it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Subject {
    /// A module, one of its dependencies, or a path through the graph.
    #[default]
    Module,
    /// Export number `n` of `from`'s exports.
    Export(u32),
    /// A file that is not a module (a `package.json`), relative to the root. No comment can
    /// suppress a violation in it.
    File(String),
    /// A file or folder of the file tree that a rule checks, relative to the root. It can be one
    /// that does not exist (a required file). Like [`Subject::File`], no comment can suppress it.
    /// `from` only orders it, and overrides scope it by this path, not by `from`.
    Path(String),
}

impl Violation {
    pub fn module(
        rule: impl Into<String>,
        level: ViolationLevel,
        from: ModuleId,
        message: String,
    ) -> Self {
        Self {
            rule: rule.into(),
            level,
            from,
            to: None,
            edge: None,
            message,
            loc: None,
            path: vec![],
            subject: Subject::Module,
            fix: None,
        }
    }
}
