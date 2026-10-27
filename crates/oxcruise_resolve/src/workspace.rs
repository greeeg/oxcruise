use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::Manifest;

#[derive(Debug, Clone)]
pub struct WorkspacePackage {
    pub name: String,
    /// Absolute directory.
    pub dir: PathBuf,
    /// Directory relative to the workspace root, `/`-separated.
    pub rel_dir: String,
}

/// npm/yarn/bun `workspaces` and `pnpm-workspace.yaml` packages.
#[derive(Debug, Clone, Default)]
pub struct Workspace {
    pub packages: Vec<WorkspacePackage>,
}

impl Workspace {
    pub fn discover(root: &Path) -> Workspace {
        let patterns = workspace_patterns(root);
        if patterns.is_empty() {
            return Workspace::default();
        }
        let (include, exclude) = build_sets(&patterns);
        let max_depth = patterns
            .iter()
            .map(|p| {
                if p.contains("**") {
                    8
                } else {
                    p.trim_matches('/').split('/').count()
                }
            })
            .max()
            .unwrap_or(1);

        let mut packages = vec![];
        let walker = WalkBuilder::new(root)
            .standard_filters(false)
            .max_depth(Some(max_depth))
            .filter_entry(|e| {
                let n = e.file_name();
                n != "node_modules" && n != ".git"
            })
            .build();
        for entry in walker.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_dir()) || entry.path() == root {
                continue;
            }
            let rel = crate::relative_to(root, entry.path());
            if !include.is_match(&rel) || exclude.is_match(&rel) {
                continue;
            }
            if let Some(m) = Manifest::read(entry.path()) {
                packages.push(WorkspacePackage {
                    name: m.name.unwrap_or_else(|| rel.clone()),
                    dir: entry.path().to_path_buf(),
                    rel_dir: rel,
                });
            }
        }
        packages.sort_by(|a, b| a.rel_dir.cmp(&b.rel_dir));
        Workspace { packages }
    }

    pub fn by_name(&self, name: &str) -> Option<usize> {
        self.packages.iter().position(|p| p.name == name)
    }

    /// Index of the package that owns `path` (deepest directory wins).
    pub fn package_of(&self, path: &Path) -> Option<usize> {
        let path = &crate::simplify_path(path);
        self.packages
            .iter()
            .enumerate()
            .filter(|(_, p)| path.starts_with(&p.dir))
            .max_by_key(|(_, p)| p.dir.components().count())
            .map(|(i, _)| i)
    }
}

fn workspace_patterns(root: &Path) -> Vec<String> {
    let mut out = vec![];
    if let Some(m) = Manifest::read(root) {
        match m.workspaces {
            Some(serde_json::Value::Array(a)) => {
                out.extend(a.iter().filter_map(|v| v.as_str().map(str::to_string)));
            }
            Some(serde_json::Value::Object(o)) => {
                if let Some(serde_json::Value::Array(a)) = o.get("packages") {
                    out.extend(a.iter().filter_map(|v| v.as_str().map(str::to_string)));
                }
            }
            _ => {}
        }
    }
    if let Ok(yaml) = std::fs::read_to_string(root.join("pnpm-workspace.yaml")) {
        out.extend(pnpm_packages(&yaml));
    }
    out
}

/// The `packages:` list of `pnpm-workspace.yaml`. Only the plain block-list
/// form is supported, which is what pnpm documents.
fn pnpm_packages(yaml: &str) -> Vec<String> {
    let mut out = vec![];
    let mut in_packages = false;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with(char::is_whitespace) && !trimmed.starts_with('-') {
            in_packages = trimmed.starts_with("packages:");
            continue;
        }
        if in_packages && let Some(item) = trimmed.strip_prefix('-') {
            let item = item
                .split(" #")
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(['"', '\'']);
            if !item.is_empty() {
                out.push(item.to_string());
            }
        }
    }
    out
}

fn build_sets(patterns: &[String]) -> (GlobSet, GlobSet) {
    let (mut inc, mut exc) = (GlobSetBuilder::new(), GlobSetBuilder::new());
    for p in patterns {
        let (neg, p) = match p.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, p.as_str()),
        };
        let p = p.trim_start_matches("./").trim_end_matches('/');
        if let Ok(g) = Glob::new(p) {
            if neg {
                exc.add(g)
            } else {
                inc.add(g)
            };
        }
    }
    (
        inc.build().unwrap_or_else(|_| GlobSet::empty()),
        exc.build().unwrap_or_else(|_| GlobSet::empty()),
    )
}

#[cfg(test)]
mod tests {
    use super::pnpm_packages;

    #[test]
    fn parses_pnpm_yaml() {
        let y = "packages:\n  - 'apps/*'\n  - \"packages/**\" # all\n  - '!**/test/**'\ncatalog:\n  x: 1\n";
        assert_eq!(
            pnpm_packages(y),
            vec!["apps/*", "packages/**", "!**/test/**"]
        );
    }
}
