//! The oxcruise command line, as a library so the napi build can reuse the same runner.

mod args;
mod config_io;
mod init;
pub mod js_plugins;
mod migrate;
mod options;
mod result;
mod rules_list;
#[cfg(all(feature = "napi", target_pointer_width = "64"))]
mod run;
mod runner;

pub use args::Cli;
pub use result::CliRunResult;
pub use runner::{Analysis, CliRunner};
