//! Language server for oxcruise: shows violations in the editor.
//!
//! The server never cruises by itself. The app hands it an [`AnalyzeFn`] that runs a full check of
//! one workspace root from disk, so config loading, plugins and rules behave exactly as on the
//! command line. Diagnostics refresh when the client opens the workspace, saves a file, or reports
//! a change to a config file.

mod code_actions;
mod diagnostics;
mod hover;
mod server;
mod source;
mod uri;

use std::path::{Path, PathBuf};

pub use lsp_server::Connection;
pub use server::serve;

use oxcruise_core::Graph;
use oxcruise_report::ViolationOut;

/// What a run found, in the form the server needs.
pub struct LspAnalysis {
    pub graph: Graph,
    pub violations: Vec<ViolationOut>,
    /// The (canonical) directory module paths are relative to.
    pub cwd: PathBuf,
}

pub struct AnalyzeOutcome {
    /// `Err` carries the message of a run that failed (bad config, missing plugin host...).
    pub analysis: Result<LspAnalysis, String>,
    /// Everything the run wrote to stderr; sent to the client log.
    pub log: String,
}

/// Runs a check of the project rooted at the given directory.
pub type AnalyzeFn = Box<dyn Fn(&Path) -> AnalyzeOutcome + Send>;
