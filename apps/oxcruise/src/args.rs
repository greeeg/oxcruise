use std::path::PathBuf;

use clap::{ArgGroup, Parser};

/// Flags that only make sense for a check; the other modes refuse them.
const CHECK_ONLY: &[&str] = &[
    "format",
    "paths",
    "quiet",
    "ignore_pattern",
    "production",
    "symbols",
    "fix",
    "max_depth",
    "max_warnings",
    "deny_warnings",
    "threads",
];

/// Flags that a check has but `--visualize` refuses: it writes a site, it does not report or gate.
const NOT_FOR_VISUALIZE: &[&str] = &["format", "fix", "quiet", "max_warnings", "deny_warnings"];

/// [`CHECK_ONLY`] without `--format`, which `--rules` takes (`-f json`).
const CHECK_ONLY_BESIDES_FORMAT: &[&str] = &[
    "paths",
    "quiet",
    "ignore_pattern",
    "production",
    "symbols",
    "fix",
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
            .args(["init", "migrate", "rules", "print_config", "visualize"])
            .multiple(false)
    )
)]
pub struct Cli {
    /// Files, directories or globs where the graph starts. The default is `entries` from the config file, or `.`
    pub paths: Vec<String>,

    /// Write a starter config file. The default is `oxcruise.config.ts`. Use `-c` to choose the file and its format
    #[arg(long, help_heading = "Mode Options", conflicts_with_all = CHECK_ONLY)]
    pub init: bool,
    /// Convert a dependency-cruiser config (found automatically, or FILE) to the `-c` file. The default target is `oxcruise.config.ts`
    #[arg(
        long,
        value_name = "FILE",
        num_args = 0..=1,
        require_equals = true,
        help_heading = "Mode Options",
        conflicts_with_all = CHECK_ONLY
    )]
    pub migrate: Option<Option<PathBuf>>,
    /// Write an interactive architecture site (graph and violations) to DIR. The default is `oxcruise-report`
    #[arg(
        long,
        value_name = "DIR",
        num_args = 0..=1,
        require_equals = true,
        help_heading = "Mode Options",
        conflicts_with_all = NOT_FOR_VISUALIZE
    )]
    pub visualize: Option<Option<PathBuf>>,
    /// List the built-in rules by category. Use `-f json` to print them as JSON
    #[arg(long, help_heading = "Mode Options", conflicts_with_all = CHECK_ONLY_BESIDES_FORMAT)]
    pub rules: bool,
    /// Print the resolved config as JSON, with `extends` applied and the level of every category
    #[arg(long, help_heading = "Mode Options", conflicts_with_all = CHECK_ONLY)]
    pub print_config: bool,

    /// The config file to read. With `--init` or `--migrate`, the file to write
    #[arg(short, long, value_name = "FILE", help_heading = "Config Options")]
    pub config: Option<PathBuf>,

    /// Leave out modules that match these globs, in addition to `ignorePatterns`. Repeat the flag for more globs
    #[arg(long, value_name = "GLOB", help_heading = "Input Options")]
    pub ignore_pattern: Vec<String>,
    /// Follow dependencies at most N hops from the entries. Deeper modules are not in the graph, so no output or rule sees them
    #[arg(long, value_name = "N", help_heading = "Input Options")]
    pub max_depth: Option<u32>,

    /// The output format: default, long, json, github, text, or plugin/reporter
    #[arg(
        short,
        long,
        value_name = "FORMAT",
        default_value = "default",
        help_heading = "Output Options"
    )]
    pub format: String,
    /// Extract exports and the names that each import uses, for `json` output and plugins.
    /// Rules that need them enable this flag
    #[arg(long, help_heading = "Input Options")]
    pub symbols: bool,

    /// Apply the automatic fixes of `unused-exports` and `unused-packages`: remove an `export`
    /// keyword, a list entry or a `package.json` entry. Run again for the fixes that these changes unlock
    #[arg(long, help_heading = "Output Options")]
    pub fix: bool,

    /// Production mode for `unused-exports`, `unused-packages` and `unused-modules`.
    /// The rules ignore tests, and they do not check `devDependencies`
    #[arg(long, help_heading = "Input Options")]
    pub production: bool,

    /// Report errors only. Warnings still count for the warning limits
    #[arg(long, help_heading = "Output Options")]
    pub quiet: bool,

    /// Exit with an error when there are more than N warnings
    #[arg(long, value_name = "N", help_heading = "Handle Warnings")]
    pub max_warnings: Option<u32>,
    /// Exit with an error when there are any warnings
    #[arg(long, help_heading = "Handle Warnings")]
    pub deny_warnings: bool,

    /// The number of worker threads. The default is all cores
    #[arg(long, value_name = "N", help_heading = "Runtime Options")]
    pub threads: Option<usize>,
    /// Run as if the command started in DIR
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
    Visualize,
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
        } else if self.visualize.is_some() {
            Mode::Visualize
        } else {
            Mode::Check
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_lists_agree() {
        assert_eq!(CHECK_ONLY[0], "format");
        assert_eq!(&CHECK_ONLY[1..], CHECK_ONLY_BESIDES_FORMAT);
    }
}
