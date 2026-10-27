use std::path::PathBuf;
use std::sync::mpsc;

use globset::Glob;
use ignore::{WalkBuilder, WalkState};

use crate::options::CruiseOptions;

/// Expand `opts.entries` into absolute, sorted, deduplicated source files.
///
/// `ignore` is applied while walking, so ignored directories are never entered.
pub fn expand_entries(opts: &CruiseOptions) -> Result<Vec<PathBuf>, String> {
    let mut out: Vec<PathBuf> = vec![];
    let mut globs = vec![];
    for e in &opts.entries {
        if e.contains(['*', '?', '[', '{']) {
            let g = Glob::new(e.strip_prefix("./").unwrap_or(e))
                .map_err(|err| format!("invalid entry glob `{e}`: {err}"))?;
            globs.push(g.compile_matcher());
            continue;
        }
        let abs = opts.root.join(e);
        if abs.is_file() {
            out.push(abs);
        } else if abs.is_dir() {
            out.extend(walk_dir(&abs, opts));
        } else {
            return Err(format!("entry `{e}` does not exist"));
        }
    }
    if !globs.is_empty() {
        out.extend(walk_dir(&opts.root, opts).into_iter().filter(|p| {
            let rel = oxcruise_resolve::relative_to(&opts.root, p);
            globs.iter().any(|g| g.is_match(&rel))
        }));
    }
    out.sort();
    out.dedup();
    out.retain(|p| {
        let rel = oxcruise_resolve::relative_to(&opts.root, p);
        !opts.ignore.is_match(&rel)
    });
    Ok(out)
}

fn walk_dir(dir: &std::path::Path, opts: &CruiseOptions) -> Vec<PathBuf> {
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
                && oxcruise_extract::is_source_path(e.path())
            {
                let _ = tx.send(e.into_path());
            }
            WalkState::Continue
        })
    });
    drop(tx);
    rx.into_iter().collect()
}
