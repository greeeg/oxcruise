use std::path::PathBuf;

use oxcruise_extract::ExtractOptions;
use oxcruise_resolve::ResolveConfig;

use crate::matcher::PathMatcher;

#[derive(Debug, Clone)]
pub struct CruiseOptions {
    /// Absolute cruise root. Module paths are reported relative to it.
    pub root: PathBuf,
    /// Files, directories or globs (relative to `root`) to start from.
    pub entries: Vec<String>,
    /// Modules matching this are not part of the graph at all.
    pub exclude: PathMatcher,
    /// If non-empty, only matching modules are part of the graph.
    pub include_only: PathMatcher,
    /// Matching modules are kept as leaves: present, but their dependencies are not explored.
    pub do_not_follow: PathMatcher,
    /// Explore at most this many hops from the entries.
    pub max_depth: Option<u32>,
    /// Parse files inside `node_modules` instead of keeping them as leaves.
    pub follow_npm: bool,
    pub respect_gitignore: bool,
    pub extract: ExtractOptions,
    pub resolve: ResolveConfig,
    /// Worker threads; `None` uses all cores.
    pub threads: Option<usize>,
}

impl CruiseOptions {
    pub fn new(root: impl Into<PathBuf>, entries: Vec<String>) -> Self {
        let root = oxcruise_resolve::simplify_path(&root.into());
        Self {
            resolve: ResolveConfig::new(root.clone()),
            root,
            entries,
            exclude: PathMatcher::default(),
            include_only: PathMatcher::default(),
            do_not_follow: PathMatcher::default(),
            max_depth: None,
            follow_npm: false,
            respect_gitignore: false,
            extract: ExtractOptions::default(),
            threads: None,
        }
    }
}
