use std::path::{Component, Path, PathBuf};

use oxc_resolver::{ResolveError, TsConfig};
use oxcruise_core::DepKinds;

use crate::Resolver;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A project file (not under `node_modules`).
    Local(PathBuf),
    /// A file inside an installed package.
    Npm {
        path: PathBuf,
        package: String,
    },
    /// A Node.js builtin, without the `node:` prefix.
    Core(String),
    Unresolved(String),
    /// Not a dependency at all (`http:`, `data:` URLs): the edge is dropped.
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub target: Target,
    /// Resolution-derived kinds: `local`, `npm-*`, `aliased-*`, `core`, `unknown`, ...
    pub kinds: DepKinds,
}

/// Builtins that Node.js has deprecated.
const DEPRECATED_CORE: &[&str] = &[
    "_linklist",
    "_stream_wrap",
    "constants",
    "domain",
    "freelist",
    "punycode",
    "smalloc",
    "sys",
];

pub(crate) fn is_skipped_specifier(spec: &str) -> bool {
    ["http:", "https:", "data:", "//", "blob:"]
        .iter()
        .any(|p| spec.starts_with(p))
}

pub(crate) fn core(resolved: &str) -> Resolved {
    let name = resolved.strip_prefix("node:").unwrap_or(resolved);
    let mut kinds = DepKinds::CORE;
    if DEPRECATED_CORE.contains(&name) {
        kinds |= DepKinds::DEPRECATED;
    }
    Resolved {
        target: Target::Core(name.to_string()),
        kinds,
    }
}

pub(crate) fn unresolved(err: &ResolveError) -> Resolved {
    Resolved {
        target: Target::Unresolved(err.to_string()),
        kinds: DepKinds::UNKNOWN,
    }
}

pub(crate) fn resolved(r: &Resolver, from_file: &Path, spec: &str, path: &Path) -> Resolved {
    let from_dir = from_file.parent().unwrap_or(from_file);

    if let Some(package) = package_from_path(path) {
        return npm(r, from_dir, spec, path, package);
    }

    let mut kinds = DepKinds::LOCAL;
    if !is_relative_or_absolute(spec) {
        kinds |= aliased_kind(r, from_file, spec);
    }
    Resolved {
        target: Target::Local(path.to_path_buf()),
        kinds,
    }
}

fn npm(r: &Resolver, from_dir: &Path, spec: &str, path: &Path, package: String) -> Resolved {
    let mut names = vec![package.clone()];
    if let Some(pkg) = bare_package_name(spec)
        && !names.contains(&pkg)
    {
        names.push(pkg);
    }
    // `@types/foo` satisfies an import of `foo` and the other way round.
    for n in names.clone() {
        names.push(types_counterpart(&n));
    }

    let mut kinds = DepKinds::empty();
    if let Some(m) = r.manifest_for(from_dir) {
        for n in &names {
            kinds |= m.sections_for(n);
        }
    }
    if kinds.is_empty() {
        kinds = DepKinds::NPM_NO_PKG;
    }
    if package_deprecated(path, &package) {
        kinds |= DepKinds::DEPRECATED;
    }
    Resolved {
        target: Target::Npm {
            path: path.to_path_buf(),
            package,
        },
        kinds,
    }
}

/// `foo` <-> `@types/foo`, `@scope/foo` <-> `@types/scope__foo`.
fn types_counterpart(name: &str) -> String {
    match name.strip_prefix("@types/") {
        Some(rest) => match rest.split_once("__") {
            Some((scope, pkg)) => format!("@{scope}/{pkg}"),
            None => rest.to_string(),
        },
        None => match name.strip_prefix('@').and_then(|n| n.split_once('/')) {
            Some((scope, pkg)) => format!("@types/{scope}__{pkg}"),
            None => format!("@types/{name}"),
        },
    }
}

fn package_deprecated(path: &Path, package: &str) -> bool {
    // <...>/node_modules/<package>/package.json
    let comps: Vec<_> = path.components().collect();
    let Some(idx) = comps
        .iter()
        .rposition(|c| matches!(c, Component::Normal(n) if *n == "node_modules"))
    else {
        return false;
    };
    let mut dir: PathBuf = comps[..=idx].iter().collect();
    dir.push(package);
    crate::Manifest::read(&dir)
        .is_some_and(|m| m.deprecated.is_some_and(|d| !d.is_null() && d != false))
}

/// `a/b/node_modules/@scope/pkg/x.js` -> `@scope/pkg` (last `node_modules` wins, which handles pnpm stores).
pub(crate) fn package_from_path(path: &Path) -> Option<String> {
    let comps: Vec<_> = path.components().collect();
    let idx = comps
        .iter()
        .rposition(|c| matches!(c, Component::Normal(n) if *n == "node_modules"))?;
    let first = comps.get(idx + 1)?.as_os_str().to_str()?;
    if first.starts_with('@') {
        let second = comps.get(idx + 2)?.as_os_str().to_str()?;
        Some(format!("{first}/{second}"))
    } else {
        Some(first.to_string())
    }
}

/// `lodash/fp` -> `lodash`, `@a/b/c` -> `@a/b`; `None` for relative or `#` specifiers.
pub(crate) fn bare_package_name(spec: &str) -> Option<String> {
    if is_relative_or_absolute(spec) || spec.starts_with('#') {
        return None;
    }
    let mut parts = spec.split('/');
    let first = parts.next()?;
    if first.starts_with('@') {
        Some(format!("{first}/{}", parts.next()?))
    } else {
        Some(first.to_string())
    }
}

fn is_relative_or_absolute(spec: &str) -> bool {
    spec == "."
        || spec == ".."
        || spec.starts_with("./")
        || spec.starts_with("../")
        || spec.starts_with('/')
}

/// Which mechanism turned a bare specifier into a project file.
fn aliased_kind(r: &Resolver, from_file: &Path, spec: &str) -> DepKinds {
    let base = DepKinds::ALIASED;
    if spec.starts_with('#') {
        return base | DepKinds::ALIASED_SUBPATH_IMPORT;
    }
    if r.config().alias.iter().any(|(k, _)| alias_matches(k, spec)) {
        return base | DepKinds::ALIASED_WEBPACK;
    }
    if let Some(name) = bare_package_name(spec)
        && r.workspace().by_name(&name).is_some()
    {
        return base | DepKinds::ALIASED_WORKSPACE;
    }
    if let Ok(Some(ts)) = r.inner().find_tsconfig(from_file) {
        if paths_match(&ts, spec) {
            return base | DepKinds::ALIASED_TSCONFIG | DepKinds::ALIASED_TSCONFIG_PATHS;
        }
        if ts.compiler_options.base_url.is_some() {
            return base | DepKinds::ALIASED_TSCONFIG | DepKinds::ALIASED_TSCONFIG_BASE_URL;
        }
    }
    base
}

fn alias_matches(key: &str, spec: &str) -> bool {
    let key = key.strip_suffix('$').unwrap_or(key);
    spec == key
        || spec
            .strip_prefix(key)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn paths_match(ts: &TsConfig, spec: &str) -> bool {
    let Some(paths) = &ts.compiler_options.paths else {
        return false;
    };
    paths.keys().any(|k| match k.split_once('*') {
        None => k == spec,
        Some((pre, post)) => {
            spec.len() >= pre.len() + post.len() && spec.starts_with(pre) && spec.ends_with(post)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_names() {
        assert_eq!(types_counterpart("react"), "@types/react");
        assert_eq!(types_counterpart("@types/react"), "react");
        assert_eq!(types_counterpart("@babel/core"), "@types/babel__core");
        assert_eq!(types_counterpart("@types/babel__core"), "@babel/core");
    }

    #[test]
    fn package_names() {
        let p = Path::new("/r/node_modules/.pnpm/a@1/node_modules/@s/pkg/x.js");
        assert_eq!(package_from_path(p).as_deref(), Some("@s/pkg"));
        assert_eq!(package_from_path(Path::new("/r/src/x.js")), None);
        assert_eq!(bare_package_name("lodash/fp").as_deref(), Some("lodash"));
        assert_eq!(bare_package_name("@a/b/c").as_deref(), Some("@a/b"));
        assert_eq!(bare_package_name("./x"), None);
        assert_eq!(bare_package_name("#int"), None);
    }
}
