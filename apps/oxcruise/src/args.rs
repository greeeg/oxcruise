use std::path::PathBuf;

use clap::{ArgGroup, Parser};

/// Flags that only make sense for a check; the file-writing modes refuse them.
const CHECK_ONLY: &[&str] = &[
    "paths",
    "format",
    "quiet",
    "ignore_pattern",
    "max_depth",
    "max_warnings",
    "deny_warnings",
    "threads",
];

#[derive(Debug, Parser)]
#[command(
    name = "oxcruise",
    version,
    about = "Validate and visualise JS/TS dependency graphs",
    group(
        ArgGroup::new("mode")
            .args(["init", "migrate", "rules", "print_config"])
            .multiple(false)
    )
)]
pub struct Cli {
    /// Files, directories or globs to start from (default: `entries` from the config, or `.`)
    pub paths: Vec<String>,

    /// Write a starter config (default oxcruise.config.ts; -c picks the file and its format)
    #[arg(long, help_heading = "Mode Options", conflicts_with_all = CHECK_ONLY)]
    pub init: bool,
    /// Convert .dependency-cruiser.* (discovered, or FILE) to the -c target (default oxcruise.config.ts)
    #[arg(
        long,
        value_name = "FILE",
        num_args = 0..=1,
        require_equals = true,
        help_heading = "Mode Options",
        conflicts_with_all = CHECK_ONLY
    )]
    pub migrate: Option<Option<PathBuf>>,
    /// List the built-in rules by category; `-f json` prints them as JSON
    #[arg(long, help_heading = "Mode Options")]
    pub rules: bool,
    /// Print the resolved config (extends applied, every category with its level) as JSON
    #[arg(long, help_heading = "Mode Options")]
    pub print_config: bool,

    /// Config to read; with --init or --migrate, the file to write
    #[arg(short, long, value_name = "FILE", help_heading = "Config Options")]
    pub config: Option<PathBuf>,

    /// Drop modules matching these globs, on top of `ignorePatterns` (repeatable)
    #[arg(long, value_name = "GLOB", help_heading = "Input Options")]
    pub ignore_pattern: Vec<String>,
    /// Follow dependencies at most this many hops from the entries (dot and mermaid output)
    #[arg(long, value_name = "N", help_heading = "Input Options")]
    pub max_depth: Option<u32>,

    /// Output format: default, long, json, github, dot, mermaid, text, or plugin/reporter
    #[arg(
        short,
        long,
        value_name = "FORMAT",
        default_value = "default",
        help_heading = "Output Options"
    )]
    pub format: String,
    /// Report errors only (warnings still count for the warning limits)
    #[arg(long, help_heading = "Output Options")]
    pub quiet: bool,

    /// Exit with an error when there are more than N warnings
    #[arg(long, value_name = "N", help_heading = "Handle Warnings")]
    pub max_warnings: Option<u32>,
    /// Exit with an error when there are any warnings
    #[arg(long, help_heading = "Handle Warnings")]
    pub deny_warnings: bool,

    /// Worker threads (default: all cores)
    #[arg(long, value_name = "N", help_heading = "Runtime Options")]
    pub threads: Option<usize>,
    /// Run as if started in DIR
    #[arg(long, value_name = "DIR", help_heading = "Runtime Options")]
    pub cwd: Option<PathBuf>,
}

/// What a run was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Init,
    Migrate,
    Rules,
    PrintConfig,
    Check,
}

impl Cli {
    pub fn mode(&self) -> Mode {
        if self.init {
            Mode::Init
        } else if self.migrate.is_some() {
            Mode::Migrate
        } else if self.rules {
            Mode::Rules
        } else if self.print_config {
            Mode::PrintConfig
        } else {
            Mode::Check
        }
    }
}
