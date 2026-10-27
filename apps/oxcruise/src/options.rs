//! Config + flags -> the options the graph runtime takes.

use std::path::{Path, PathBuf};

use oxcruise_config::{OneOrMany, OxcruiseConfig};
use oxcruise_graph::{CruiseOptions, PathMatcher};
use oxcruise_resolve::TsconfigMode;

use crate::args::CheckArgs;

pub fn build(root: &Path, cfg: &OxcruiseConfig, args: &CheckArgs) -> Result<CruiseOptions, String> {
    let entries = if !args.paths.is_empty() {
        args.paths.clone()
    } else if !cfg.entries.is_empty() {
        cfg.entries.clone()
    } else {
        vec![".".to_string()]
    };

    let mut o = CruiseOptions::new(root, entries);
    let ignore: Vec<&String> = cfg
        .ignore_patterns
        .iter()
        .chain(&args.ignore_pattern)
        .collect();
    o.ignore = PathMatcher::new(&ignore).map_err(|e| format!("ignorePatterns: {e}"))?;
    o.do_not_follow =
        PathMatcher::new(&cfg.do_not_follow).map_err(|e| format!("doNotFollow: {e}"))?;
    o.max_depth = args.max_depth;
    o.threads = args.threads;
    o.extract.calls = cfg.calls.clone();

    let r = &cfg.resolve;
    o.resolve.conditions = r.conditions.clone();
    o.resolve.tsconfig = match &r.tsconfig {
        None => TsconfigMode::Auto,
        Some(p) => TsconfigMode::File(root.join(p)),
    };
    o.resolve.alias = r
        .alias
        .iter()
        .map(|(k, v)| {
            let targets = match v {
                OneOrMany::One(t) => vec![t.clone()],
                OneOrMany::Many(ts) => ts.clone(),
            };
            (
                k.clone(),
                targets.into_iter().map(|t| absolutize(root, &t)).collect(),
            )
        })
        .collect();
    Ok(o)
}

/// `./src` -> `<root>/src`; bare package names and absolute paths are left alone.
fn absolutize(root: &Path, target: &str) -> String {
    if target.starts_with('.') {
        let joined: PathBuf = root.join(target);
        joined
            .components()
            .fold(PathBuf::new(), |mut acc, c| {
                match c {
                    std::path::Component::ParentDir => {
                        acc.pop();
                    }
                    std::path::Component::CurDir => {}
                    other => acc.push(other),
                }
                acc
            })
            .to_string_lossy()
            .into_owned()
    } else {
        target.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolutizes_relative_alias_targets() {
        // Compared as paths: Windows renders the result with backslashes.
        let root = Path::new("/repo");
        assert_eq!(
            Path::new(&absolutize(root, "./src")),
            Path::new("/repo/src")
        );
        assert_eq!(
            Path::new(&absolutize(root, "../shared/lib")),
            Path::new("/shared/lib")
        );
        assert_eq!(absolutize(root, "some-package"), "some-package");
        assert_eq!(absolutize(root, "/abs"), "/abs");
    }
}
