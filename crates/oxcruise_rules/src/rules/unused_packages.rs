use oxcruise_core::{DeclaredDep, DepKinds, DepSection, Fix, ModuleId, ModuleKind};
use oxcruise_macros::declare_rule;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;

use super::is_test_module;
use crate::conventions::{self, ConventionFilter};
use crate::meta::{Analyses, Hooks, Rule, RuleCtx, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NoUnusedDepsConfig {
    /// The `package.json` sections to check (default `dependencies`, `peerDependencies` and
    /// `optionalDependencies`; `devDependencies` are mostly tools that are never imported).
    pub sections: Vec<DepSectionName>,
    /// Globs of package names that are never reported.
    pub ignore: Vec<String>,
    /// Whether packages that the tools you use load by name count as used (`@vitest/*` with
    /// `vitest`): `true` (all, the default), `false`, or the names of the conventions to use.
    pub conventions: ConventionFilter,
    /// Production mode: `devDependencies` are not checked and tests do not use anything.
    pub production: bool,
    /// Globs of test modules, on top of `*.test.*`, `*.spec.*` and test directories.
    pub test: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DepSectionName {
    Dependencies,
    DevDependencies,
    PeerDependencies,
    OptionalDependencies,
}

impl From<DepSectionName> for DepSection {
    fn from(s: DepSectionName) -> Self {
        match s {
            DepSectionName::Dependencies => DepSection::Dependencies,
            DepSectionName::DevDependencies => DepSection::DevDependencies,
            DepSectionName::PeerDependencies => DepSection::PeerDependencies,
            DepSectionName::OptionalDependencies => DepSection::OptionalDependencies,
        }
    }
}

impl Default for NoUnusedDepsConfig {
    fn default() -> Self {
        Self {
            sections: vec![
                DepSectionName::Dependencies,
                DepSectionName::PeerDependencies,
                DepSectionName::OptionalDependencies,
            ],
            ignore: vec![],
            conventions: ConventionFilter::default(),
            production: false,
            test: vec![],
        }
    }
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports a package that a `package.json` declares and that none of its modules imports.
    ///
    /// ### Why is this bad?
    ///
    /// An unused dependency costs install time and audit work, and it ships for no reason.
    ///
    /// ### How it works
    ///
    /// A package counts as used when a module that the `package.json` governs imports it.
    /// The `package.json` that governs a module is the nearest one above the module.
    /// The subpath of the import does not matter. These packages also count as used:
    ///
    /// * `@types/x` follows `x`.
    /// * `@types/node` follows any built-in module.
    /// * The peer dependencies of a used package.
    ///
    /// The rule does not see two kinds of use. A script can only run a package, for example a CLI.
    /// The configuration of a tool can load a package.
    /// Add these packages to `ignore`.
    /// The rule skips a `package.json` that governs no module of the graph.
    ///
    /// ### Options
    ///
    /// * `production`: do not check `devDependencies`, and ignore the imports of tests.
    ///   `oxcruise --production` enables it.
    /// * `test`: globs of test modules. These globs add to `*.test.*`, `*.spec.*` and test directories.
    /// * `sections`: the `package.json` sections to check.
    ///   The default is `dependencies`, `peerDependencies` and `optionalDependencies`.
    ///   Most `devDependencies` are tools that no module imports.
    /// * `ignore`: globs of package names to skip.
    /// * `conventions`: packages that a tool loads by name count as used when the same `package.json` lists the tool.
    ///   For example `@vitest/*` next to `vitest`, and `*-loader` next to `webpack`.
    ///   The default is `true` (all conventions). Use `false` to disable them, or a list of names to keep only those conventions.
    ///
    /// A violation of this rule is in a `package.json`, and a comment cannot suppress it there.
    /// Use `ignore` instead.
    ///
    /// ### Fix
    ///
    /// This rule is fixable. `oxcruise --fix` removes the entry from the `package.json`.
    NoUnusedDeps, category = nursery, version = "0.1.0", config = NoUnusedDepsConfig
}

#[derive(Debug, Default)]
pub struct NoUnusedDeps {
    sections: Vec<DepSection>,
    ignore: Option<PathPatterns>,
    conventions: ConventionFilter,
    /// Test modules, in production mode.
    test: Option<PathPatterns>,
}

/// The byte range to delete for each unused dependency so that its section stays valid JSON, and
/// so that the ranges of neighbours touch without overlapping (all of them are fixed at once).
/// A dependency that a kept one follows takes its own member and the comma after it; the unused
/// ones at the end of a section take the comma before them instead.
fn fix_ranges(deps: &[DeclaredDep], unused: &[bool]) -> Vec<(u32, u32)> {
    let mut ranges: Vec<(u32, u32)> = deps.iter().map(|d| d.range).collect();
    for section in [
        DepSection::Dependencies,
        DepSection::DevDependencies,
        DepSection::PeerDependencies,
        DepSection::OptionalDependencies,
    ] {
        let members: Vec<usize> = (0..deps.len())
            .filter(|&i| deps[i].section == section)
            .collect();
        // the last member that stays; what follows it is the trailing run
        let last_kept = members.iter().rposition(|&i| !unused[i]);
        for (pos, &i) in members.iter().enumerate() {
            if !unused[i] {
                continue;
            }
            ranges[i] = match (last_kept, pos.checked_sub(1)) {
                (Some(kept), _) if pos < kept => (deps[i].range.0, deps[members[pos + 1]].range.0),
                (_, Some(prev)) => (deps[members[prev]].range.1, deps[i].range.1),
                (_, None) => deps[i].range,
            };
        }
    }
    ranges
}

/// `@scope/name/sub/path` -> `@scope/name`, `name/sub` -> `name`.
fn package_name(specifier: &str) -> &str {
    let mut parts = specifier.splitn(if specifier.starts_with('@') { 3 } else { 2 }, '/');
    let first = parts.next().unwrap_or(specifier);
    match (specifier.starts_with('@'), parts.next()) {
        (true, Some(second)) => &specifier[..first.len() + 1 + second.len()],
        _ => first,
    }
}

/// `@types/scope__name` -> `@scope/name`, `@types/name` -> `name`.
fn types_target(name: &str) -> Option<String> {
    let rest = name.strip_prefix("@types/")?;
    Some(match rest.split_once("__") {
        Some((scope, pkg)) => format!("@{scope}/{pkg}"),
        None => rest.to_string(),
    })
}

impl Rule for NoUnusedDeps {
    const HOOKS: Hooks = Hooks::GRAPH;

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: NoUnusedDepsConfig = parse_options("no-unused-deps", value)?;
        c.conventions.validate()?;
        Ok(Self {
            conventions: c.conventions,
            sections: c
                .sections
                .into_iter()
                .map(DepSection::from)
                .filter(|s| !(c.production && *s == DepSection::DevDependencies))
                .collect(),
            test: c
                .production
                .then(|| PathPatterns::new(&c.test, Role::Referencing))
                .transpose()?,
            ignore: Some(PathPatterns::new(&c.ignore, Role::Referencing)?),
        })
    }

    fn has_production_option() -> bool {
        true
    }

    fn requirements(&self) -> Analyses {
        Analyses::MANIFESTS | Analyses::DEP_PEERS
    }

    fn check_graph(&self, cx: &RuleCtx<'_>) {
        let g = cx.graph();
        let manifests = cx.manifests();
        let mut used: Vec<FxHashSet<&str>> = vec![FxHashSet::default(); manifests.len()];
        let mut owner: Vec<Option<ModuleId>> = vec![None; manifests.len()];
        for m in g.module_ids() {
            let module = g.module(m);
            if module.kind != ModuleKind::Local {
                continue;
            }
            if let Some(i) = module.manifest {
                owner[i as usize].get_or_insert(m);
            }
        }
        for e in g.edges() {
            let Some(i) = g.module(e.from).manifest else {
                continue;
            };
            if let Some(test) = &self.test
                && is_test_module(&g.module(e.from).path, test)
            {
                continue;
            }
            let bare = !e.specifier.starts_with(['.', '/']);
            if e.kinds.contains(DepKinds::CORE) {
                used[i as usize].insert("@types/node");
            } else if e
                .kinds
                .intersects(DepKinds::NPM_ANY | DepKinds::ALIASED_WORKSPACE)
                || (e.kinds.contains(DepKinds::UNKNOWN) && bare)
            {
                used[i as usize].insert(package_name(&e.specifier));
            }
        }

        for (i, manifest) in manifests.iter().enumerate() {
            let Some(first_module) = owner[i] else {
                continue;
            };
            // `@types/x` follows `x`, and a used package brings the peers it needs
            let mut live: FxHashSet<&str> = used[i].clone();
            let mut queue: Vec<&str> = live.iter().copied().collect();
            while let Some(name) = queue.pop() {
                for d in manifest.deps.iter().filter(|d| d.name == name) {
                    for peer in &d.peers {
                        if live.insert(peer) {
                            queue.push(peer);
                        }
                    }
                }
            }
            let filters = [self.conventions.clone()];
            let uses: Vec<String> = conventions::active(manifest, &filters)
                .flat_map(|c| c.uses.iter().cloned())
                .collect();
            let uses = PathPatterns::new(&uses, Role::Referencing).ok();
            let unused: Vec<bool> = manifest
                .deps
                .iter()
                .map(|d| {
                    !uses
                        .as_ref()
                        .is_some_and(|u| u.is_match(&d.name, &Captures::default()))
                        && self.sections.contains(&d.section)
                        && !live.contains(d.name.as_str())
                        && !types_target(&d.name).is_some_and(|t| live.contains(t.as_str()))
                        && !self
                            .ignore
                            .as_ref()
                            .is_some_and(|ignore| ignore.is_match(&d.name, &Captures::default()))
                })
                .collect();
            let ranges = fix_ranges(&manifest.deps, &unused);
            for (k, d) in manifest.deps.iter().enumerate() {
                if !unused[k] {
                    continue;
                }
                let fix = Some(ranges[k]).filter(|r| r.1 > r.0).map(|(s, e)| Fix {
                    file: manifest.rel_path.clone(),
                    edits: vec![(s, e, String::new())],
                });
                cx.report_file(
                    first_module,
                    &manifest.rel_path,
                    d.loc,
                    format!(
                        "`{}` is in `{}` but no module imports it",
                        d.name,
                        d.section.as_str()
                    ),
                    fix,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use oxcruise_core::Loc;
    use oxcruise_resolve::scan_dependency_members;

    use super::*;

    /// Remove the unused dependencies (`unused[i]` for the i-th member) in one go.
    fn fixed(text: &str, unused: &[bool]) -> String {
        let deps: Vec<DeclaredDep> = scan_dependency_members(text)
            .into_iter()
            .map(|(section, name, start, end)| DeclaredDep {
                name,
                section,
                loc: Loc::default(),
                range: (start as u32, end as u32),
                peers: vec![],
            })
            .collect();
        assert_eq!(deps.len(), unused.len());
        let mut ranges: Vec<_> = fix_ranges(&deps, unused)
            .into_iter()
            .zip(unused)
            .filter(|(_, u)| **u)
            .map(|(r, _)| r)
            .collect();
        ranges.sort();
        for pair in ranges.windows(2) {
            assert!(pair[0].1 <= pair[1].0, "overlap in {ranges:?}");
        }
        let mut out = text.to_string();
        for (s, e) in ranges.into_iter().rev() {
            out.replace_range(s as usize..e as usize, "");
        }
        out
    }

    fn names(json: &str) -> Vec<String> {
        let v: serde_json::Value =
            serde_json::from_str(json).unwrap_or_else(|e| panic!("not valid JSON ({e}): {json}"));
        v["dependencies"]
            .as_object()
            .map(|o| o.keys().cloned().collect())
            .unwrap_or_default()
    }

    #[test]
    fn neighbouring_unused_dependencies_are_removed_in_one_run() {
        let text = "{\n  \"dependencies\": {\n    \"a\": \"1\",\n    \"b\": \"1\",\n    \"c\": \"1\"\n  },\n  \"name\": \"p\"\n}\n";
        // every subset of {a, b, c} leaves valid JSON with exactly the kept members
        for mask in 0..8u8 {
            let unused: Vec<bool> = (0..3).map(|i| mask & (1 << i) != 0).collect();
            let out = fixed(text, &unused);
            let kept: Vec<String> = ["a", "b", "c"]
                .iter()
                .zip(&unused)
                .filter(|(_, u)| !**u)
                .map(|(n, _)| n.to_string())
                .collect();
            assert_eq!(names(&out), kept, "mask {mask:03b}: {out}");
        }
    }

    #[test]
    fn a_section_on_one_line_and_other_sections_stay_valid() {
        let text =
            r#"{"dependencies":{"a":"1","b":"1","c":"1"},"devDependencies":{"d":"1","e":"1"}}"#;
        let out = fixed(text, &[false, true, true, true, true]);
        assert_eq!(names(&out), ["a"]);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(v["devDependencies"].as_object().unwrap().is_empty());
    }
}
