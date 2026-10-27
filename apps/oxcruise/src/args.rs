use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "oxcruise",
    version,
    about = "Validate and visualise JS/TS dependency graphs",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[command(flatten)]
    pub check: CheckArgs,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check the dependency graph against the rules (the default)
    Check(CheckArgs),
    /// Convert a .dependency-cruiser.* config to an oxcruise config, with warnings for what does not map
    Migrate(MigrateArgs),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Args, Default)]
pub struct CheckArgs {
    /// Files, directories or globs to start from (default: `entries` from the config, or `.`)
    pub paths: Vec<String>,

    /// Config file (default: oxcruise.config.ts or .oxcruiserc.json in the working directory)
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// Ignore config files and use the defaults
    #[arg(long)]
    pub no_config: bool,

    /// Output format: default, long, json, github, dot, mermaid, text, or plugin/reporter
    #[arg(short, long, value_name = "FORMAT", default_value = "default")]
    pub format: String,
    /// Write the report to a file instead of stdout
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Drop modules matching these globs, on top of `ignorePatterns` (repeatable)
    #[arg(long, value_name = "GLOB")]
    pub ignore_pattern: Vec<String>,
    /// Follow dependencies at most this many hops from the entries (dot and mermaid output)
    #[arg(long, value_name = "N")]
    pub max_depth: Option<u32>,

    /// Exit with an error when there are more than N warnings
    #[arg(long, value_name = "N")]
    pub max_warnings: Option<u32>,
    /// Exit with an error when there are any warnings
    #[arg(long)]
    pub deny_warnings: bool,

    /// Worker threads (default: all cores)
    #[arg(long, value_name = "N")]
    pub threads: Option<usize>,
    /// Print the built-in rules as JSON and exit
    #[arg(long)]
    pub rules: bool,
    #[arg(long, value_enum, default_value = "auto")]
    pub color: ColorChoice,
    /// Run as if started in DIR
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct MigrateArgs {
    /// The dependency-cruiser config to read (default: discover .dependency-cruiser.* in the cwd)
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,
    /// Where to write (default: oxcruise.config.ts, or .oxcruiserc.json with --json)
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,
    /// Write a JSON config instead of a TypeScript one
    #[arg(long)]
    pub json: bool,
    /// Overwrite the output file if it exists
    #[arg(long)]
    pub force: bool,
    #[arg(long, value_name = "DIR")]
    pub cwd: Option<PathBuf>,
}
