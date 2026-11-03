//! What the tools a project uses load by convention, as data (`conventions.json`).
//!
//! A convention is active for a `package.json` when it declares one of the convention's `when`
//! packages (none: always). Its `entries` are files the tool loads without anyone importing them
//! (tests, routes, configs), and its `uses` are packages the tool loads by name. It is Knip's
//! plugin idea without the plugin code.

use std::sync::LazyLock;

use oxcruise_core::DeclaredManifest;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::pattern::{PathPatterns, Role};

#[derive(Debug, Deserialize)]
pub struct Convention {
    pub name: String,
    /// Package names; active when the manifest declares any of them. Empty: always active.
    pub when: Vec<String>,
    /// Globs relative to the manifest's directory.
    pub entries: Vec<String>,
    /// Package-name globs counted as used while the convention is active.
    pub uses: Vec<String>,
}

static TABLE: LazyLock<Vec<Convention>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("conventions.json")).expect("conventions.json is valid")
});

/// Every convention.
pub fn all() -> &'static [Convention] {
    &TABLE
}

/// Which conventions a rule takes into account: `true` (all, the default), `false` (none) or the
/// names of the ones to use.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ConventionFilter {
    Switch(bool),
    Only(Vec<String>),
}

impl Default for ConventionFilter {
    fn default() -> Self {
        ConventionFilter::Switch(true)
    }
}

impl ConventionFilter {
    pub fn allows(&self, name: &str) -> bool {
        match self {
            ConventionFilter::Switch(on) => *on,
            ConventionFilter::Only(names) => names.iter().any(|n| n == name),
        }
    }

    /// Unknown names are mistakes worth reporting.
    pub fn validate(&self) -> Result<(), String> {
        if let ConventionFilter::Only(names) = self
            && let Some(bad) = names.iter().find(|n| !all().iter().any(|c| &c.name == *n))
        {
            return Err(format!(
                "unknown convention `{bad}`; known: {}",
                all()
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        Ok(())
    }
}

/// The conventions that apply to `manifest` and that some filter allows.
pub fn active<'a>(
    manifest: &'a DeclaredManifest,
    filters: &'a [ConventionFilter],
) -> impl Iterator<Item = &'static Convention> + 'a {
    all().iter().filter(move |c| {
        filters.iter().any(|f| f.allows(&c.name))
            && (c.when.is_empty() || c.when.iter().any(|w| manifest.declared.contains(w)))
    })
}

/// The entry globs of `conventions` below `manifest`'s directory.
pub fn entry_patterns(
    manifest: &DeclaredManifest,
    conventions: &[&Convention],
) -> Result<PathPatterns, String> {
    let prefix = if manifest.rel_dir.is_empty() {
        String::new()
    } else {
        format!("{}/", manifest.rel_dir)
    };
    let globs: Vec<String> = conventions
        .iter()
        .flat_map(|c| c.entries.iter().map(|g| format!("{prefix}{g}")))
        .collect();
    PathPatterns::new(&globs, Role::Referencing)
}

/// `(name, entries, uses)` for the docs table.
pub fn table() -> impl Iterator<Item = &'static Convention> {
    all().iter()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::pattern::Captures;

    #[test]
    fn the_table_is_well_formed() {
        let mut names = HashSet::new();
        for c in all() {
            assert!(names.insert(&c.name), "duplicate convention {}", c.name);
            let mut seen = HashSet::new();
            assert!(
                c.when.iter().all(|w| seen.insert(w)),
                "{}: duplicate when",
                c.name
            );
            for g in c.entries.iter().chain(&c.uses) {
                PathPatterns::new(std::slice::from_ref(g), Role::Referencing)
                    .unwrap_or_else(|e| panic!("{}: {e}", c.name));
            }
        }
        assert!(all().len() >= 25);
    }

    fn manifest(dir: &str, declared: &[&str]) -> DeclaredManifest {
        DeclaredManifest {
            rel_path: format!("{dir}/package.json"),
            rel_dir: dir.to_string(),
            name: None,
            deps: vec![],
            declared: declared.iter().map(|s| s.to_string()).collect(),
            entry_files: vec![],
            out_dir: None,
            root_dir: None,
        }
    }

    #[test]
    fn conventions_follow_the_dependencies() {
        let m = manifest("web", &["vitest", "next"]);
        let filters = [ConventionFilter::default()];
        let names: Vec<_> = active(&m, &filters).map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["vitest", "node-test", "next"]);

        let chosen: Vec<_> = active(&m, &filters).collect();
        let p = entry_patterns(&m, &chosen).unwrap();
        let none = Captures::default();
        assert!(p.is_match("web/app/page.tsx", &none));
        assert!(p.is_match("web/app/blog/[slug]/page.tsx", &none));
        assert!(p.is_match("web/src/x.test.ts", &none));
        assert!(p.is_match("web/next.config.ts", &none));
        assert!(p.is_match("web/src/middleware.ts", &none));
        assert!(!p.is_match("web/app/components/button.tsx", &none));
        assert!(!p.is_match("other/app/page.tsx", &none));

        let only = [ConventionFilter::Only(vec!["next".into()])];
        let names: Vec<_> = active(&m, &only).map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["next"]);
        assert_eq!(active(&m, &[ConventionFilter::Switch(false)]).count(), 0);
        assert!(
            ConventionFilter::Only(vec!["nope".into()])
                .validate()
                .is_err()
        );
    }
}
