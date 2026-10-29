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
    /// Modules matching this are not part of the graph at all. `.gitignore` is always honoured
    /// and `node_modules` are always leaves.
    pub ignore: PathMatcher,
    /// Matching modules are kept as leaves: present, but their dependencies are not explored.
    pub do_not_follow: PathMatcher,
    /// Explore at most this many hops from the entries.
    pub max_depth: Option<u32>,
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
            ignore: PathMatcher::default(),
            do_not_follow: PathMatcher::default(),
            max_depth: None,
            extract: ExtractOptions::default(),
            threads: None,
        }
    }
}
