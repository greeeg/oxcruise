use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use globset::Glob;
use ignore::{WalkBuilder, WalkState};

use crate::options::CruiseOptions;

/// What the walk of the entries found.
#[derive(Debug, Default)]
pub struct Expanded {
    /// Absolute, sorted, deduplicated source files: where the graph starts.
    pub sources: Vec<PathBuf>,
    /// With `CruiseOptions::file_tree`: every file the walk saw, source or not, absolute. Not
    /// sorted, and a file can be in it twice. Empty otherwise.
    pub files: Vec<PathBuf>,
    /// With `CruiseOptions::file_tree`: folders that exist but whose content is not in `files`,
    /// absolute. These are the folders next to an entry folder and above it.
    pub unlisted: Vec<PathBuf>,
}

/// Expand `opts.entries` into source files, and with `opts.file_tree` into every file below them.
///
/// `ignore` is applied while walking, so ignored directories are never entered. There is one walk
/// per entry folder. If no entry is a folder, or an entry is a glob, the tree comes from one walk
/// of the root. Otherwise the folders above the entry folders are listed one level deep: their
/// own files are in the tree, and their other folders are `unlisted`.
pub fn expand_entries(opts: &CruiseOptions) -> Result<Expanded, String> {
    let mut out = Expanded::default();
    let mut globs = vec![];
    let mut walked_root = false;
    let mut entry_files = vec![];
    let mut entry_dirs = vec![];
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
                out.files.push(abs.clone());
                entry_files.push(abs);
            }
        } else if abs.is_dir() {
            walked_root |= abs == opts.root;
            entry_dirs.push(abs.clone());
            let (sources, files) = walk_dir(&abs, opts);
            out.sources.extend(sources);
            out.files.extend(files);
        } else {
            return Err(format!("entry `{e}` does not exist"));
        }
    }
    let whole_root = !globs.is_empty() || (opts.file_tree && entry_dirs.is_empty());
    if whole_root {
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
    if opts.file_tree && !whole_root && !walked_root {
        list_above(&entry_dirs, &entry_files, opts, &mut out);
    }
    Ok(out)
}

/// The folders above the entries, one level deep: their files go to `out.files`, and their folders
/// that no entry covers to `out.unlisted`. A folder with an entry folder in it, or an entry file,
/// is listed; one below an entry folder is already walked.
fn list_above(
    entry_dirs: &[PathBuf],
    entry_files: &[PathBuf],
    opts: &CruiseOptions,
    out: &mut Expanded,
) {
    let mut above: BTreeSet<&Path> = BTreeSet::new();
    let strict = entry_dirs.iter().flat_map(|d| d.ancestors().skip(1));
    let parents = entry_files.iter().flat_map(|f| f.ancestors().skip(1));
    for a in strict.chain(parents) {
        if a.starts_with(&opts.root) && !entry_dirs.iter().any(|d| a.starts_with(d)) {
            above.insert(a);
        }
    }
    for dir in &above {
        let (files, dirs) = list_dir(dir, opts);
        out.files.extend(files);
        out.unlisted.extend(dirs.into_iter().filter(|d| {
            !above.contains(d.as_path())
                && !entry_dirs
                    .iter()
                    .any(|e| d.starts_with(e) || e.starts_with(d))
        }));
    }
}

/// The files and folders directly in `dir`, with the same filters as the walk.
fn list_dir(dir: &Path, opts: &CruiseOptions) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let (mut files, mut dirs) = (vec![], vec![]);
    for entry in walk_builder(dir, opts).max_depth(Some(1)).build().flatten() {
        if entry.depth() == 0 {
            continue;
        }
        match entry.file_type() {
            Some(t) if t.is_file() => files.push(entry.into_path()),
            Some(t) if t.is_dir() => dirs.push(entry.into_path()),
            _ => {}
        }
    }
    (files, dirs)
}

fn walk_builder(dir: &Path, opts: &CruiseOptions) -> WalkBuilder {
    let root = opts.root.clone();
    let ignore = opts.ignore.clone();
    let mut builder = WalkBuilder::new(dir);
    builder
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
        });
    builder
}

/// `(source files, all files)` below `dir`. The second list is empty without `opts.file_tree`.
fn walk_dir(dir: &Path, opts: &CruiseOptions) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let tree = opts.file_tree;
    let walker = walk_builder(dir, opts).build_parallel();

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
