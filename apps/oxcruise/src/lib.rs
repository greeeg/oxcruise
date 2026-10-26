//! The oxcruise command line, as a library so the napi build can reuse the same runner.

mod args;
mod config_io;
mod migrate;
mod options;
mod result;
mod runner;

pub use args::{CheckArgs, Cli, ColorChoice, Command};
pub use result::CliRunResult;
pub use runner::CliRunner;
