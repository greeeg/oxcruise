//! Config + flags -> the options the graph runtime takes.

use std::path::{Path, PathBuf};

use oxcruise_config::{OneOrMany, OxcruiseConfig, PathSpec, TsconfigMode, TsconfigSetting};
use oxcruise_graph::{CruiseOptions, PathMatcher, PathPattern};
use oxcruise_resolve::TsconfigMode as ResolveTsconfig;

use crate::args::CheckArgs;

fn patterns(specs: &[PathSpec], extra_globs: &[String]) -> Result<PathMatcher, String> {
    let mut all: Vec<PathPattern> = specs
        .iter()
        .map(|s| match s {
            PathSpec::Glob(g) => PathPattern::Glob(g.clone()),
            PathSpec::Regex(r) => PathPattern::Regex(r.regex.clone()),
        })
        .collect();
    all.extend(extra_globs.iter().cloned().map(PathPattern::Glob));
    PathMatcher::new(&all)
}

pub fn build(root: &Path, cfg: &OxcruiseConfig, args: &CheckArgs) -> Result<CruiseOptions, String> {
    let entries = if !args.paths.is_empty() {
        args.paths.clone()
    } else if !cfg.entries.is_empty() {
        cfg.entries.clone()
    } else {
        vec![".".to_string()]
    };

    let mut o = CruiseOptions::new(root, entries);
    o.exclude = patterns(&cfg.exclude, &args.exclude).map_err(|e| format!("exclude: {e}"))?;
    o.include_only =
        patterns(&cfg.include_only, &args.include_only).map_err(|e| format!("includeOnly: {e}"))?;
    o.do_not_follow = patterns(&cfg.follow.do_not_follow, &args.do_not_follow)
        .map_err(|e| format!("follow.doNotFollow: {e}"))?;
    o.max_depth = args.max_depth.or(cfg.follow.max_depth);
    o.follow_npm = cfg.follow.npm;
    o.respect_gitignore = cfg.options.respect_gitignore;
    o.threads = args.threads;

    let e = &cfg.extract;
    o.extract.exotic_require_strings = e.exotic_require_strings.clone();
    o.extract.calls = e.calls.clone();
    o.extract.jsdoc = e.jsdoc;
    o.extract.skip_triple_slash = e.skip_triple_slash;

    let r = &cfg.resolve;
    o.resolve.conditions = r.conditions.clone();
    o.resolve.main_fields = (!r.main_fields.is_empty()).then(|| r.main_fields.clone());
    o.resolve.extensions = (!r.extensions.is_empty()).then(|| r.extensions.clone());
    o.resolve.prefer_source = r.prefer_source;
    if !r.source_conditions.is_empty() {
        o.resolve.source_conditions = r.source_conditions.clone();
    }
    o.resolve.tsconfig = match &r.tsconfig {
        None | Some(TsconfigSetting::Mode(TsconfigMode::Auto)) => ResolveTsconfig::Auto,
        Some(TsconfigSetting::Mode(TsconfigMode::Off)) => ResolveTsconfig::Disabled,
        Some(TsconfigSetting::Path(p)) => ResolveTsconfig::File(root.join(p)),
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
        let root = Path::new("/repo");
        assert_eq!(absolutize(root, "./src"), "/repo/src");
        assert_eq!(absolutize(root, "../shared/lib"), "/shared/lib");
        assert_eq!(absolutize(root, "some-package"), "some-package");
        assert_eq!(absolutize(root, "/abs"), "/abs");
    }
}
