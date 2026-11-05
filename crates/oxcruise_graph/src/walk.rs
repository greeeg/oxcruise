use std::path::PathBuf;
use std::sync::mpsc;

use globset::Glob;
use ignore::{WalkBuilder, WalkState};

use crate::options::CruiseOptions;

/// What the walk of the entries found.
#[derive(Debug, Default)]
pub struct Expanded {
    /// Absolute, sorted, deduplicated source files: where the graph starts.
    pub sources: Vec<PathBuf>,
    /// With `CruiseOptions::file_tree`: every file the walk saw, source or not, absolute and
    /// sorted. Empty otherwise.
    pub files: Vec<PathBuf>,
}

/// Expand `opts.entries` into source files, and with `opts.file_tree` into every file below them.
///
/// `ignore` is applied while walking, so ignored directories are never entered. There is one walk
/// per entry folder. If no entry is a folder, the tree comes from one walk of the root.
pub fn expand_entries(opts: &CruiseOptions) -> Result<Expanded, String> {
    let mut out = Expanded::default();
    let mut globs = vec![];
    let mut walked_root = false;
    for e in &opts.entries {
        if e.contains(['*', '?', '[', '{']) {
            let g = Glob::new(e.strip_prefix("./").unwrap_or(e))
                .map_err(|err| format!("invalid entry glob `{e}`: {err}"))?;
            globs.push(g.compile_matcher());
            continue;
        }
        let abs = opts.root.join(e);
        if abs.is_file() {
            out.sources.push(abs.clone());
            if opts.file_tree {
                out.files.push(abs);
            }
        } else if abs.is_dir() {
            walked_root |= abs == opts.root;
            let (sources, files) = walk_dir(&abs, opts);
            out.sources.extend(sources);
            out.files.extend(files);
        } else {
            return Err(format!("entry `{e}` does not exist"));
        }
    }
    let entry_folders = opts.entries.iter().any(|e| opts.root.join(e).is_dir());
    if !globs.is_empty() || (opts.file_tree && !entry_folders) {
        let (sources, files) = walk_dir(&opts.root, opts);
        out.sources.extend(sources.into_iter().filter(|p| {
            let rel = oxcruise_resolve::relative_to(&opts.root, p);
            globs.iter().any(|g| g.is_match(&rel))
        }));
        // an entry that is the root has walked it already
        if !walked_root {
            out.files.extend(files);
        }
    }
    out.sources.sort();
    out.sources.dedup();
    out.sources.retain(|p| {
        let rel = oxcruise_resolve::relative_to(&opts.root, p);
        !opts.ignore.is_match(&rel)
    });
    out.files.sort();
    out.files.dedup();
    Ok(out)
}

/// `(source files, all files)` below `dir`. The second list is empty without `opts.file_tree`.
fn walk_dir(dir: &std::path::Path, opts: &CruiseOptions) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let tree = opts.file_tree;
    let root = opts.root.clone();
    let ignore = opts.ignore.clone();
    let walker = WalkBuilder::new(dir)
        .standard_filters(false)
        .git_ignore(true)
        .require_git(false)
        .threads(opts.threads.unwrap_or(0))
        .filter_entry(move |e| {
            let name = e.file_name();
            if name == "node_modules" || name == ".git" {
                return false;
            }
            let rel = oxcruise_resolve::relative_to(&root, e.path());
            !ignore.is_match(&rel)
        })
        .build_parallel();

    let (tx, rx) = mpsc::channel();
    walker.run(|| {
        let tx = tx.clone();
        Box::new(move |entry| {
            if let Ok(e) = entry
                && e.file_type().is_some_and(|t| t.is_file())
            {
                let source = oxcruise_extract::is_source_path(e.path());
                if source || tree {
                    let _ = tx.send((e.into_path(), source));
                }
            }
            WalkState::Continue
        })
    });
    drop(tx);
    let (mut sources, mut files) = (vec![], vec![]);
    for (path, source) in rx {
        if tree {
            files.push(path.clone());
        }
        if source {
            sources.push(path);
        }
    }
    (sources, files)
}
