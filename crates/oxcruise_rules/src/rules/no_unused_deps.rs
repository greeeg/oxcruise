use oxcruise_core::{DepKinds, DepSection, Fix, ModuleId, ModuleKind};
use oxcruise_macros::declare_rule;
use rustc_hash::FxHashSet;
use schemars::JsonSchema;
use serde::Deserialize;

use super::is_test_module;
use crate::conventions::{self, ConventionFilter};
use crate::meta::{Analyses, Rule, RuleCtx, parse_options};
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
    /// Reports packages that a `package.json` declares and none of its modules imports.
    ///
    /// ### Why is this bad?
    ///
    /// An unused dependency is installed, audited and shipped for nothing.
    ///
    /// ### How it works
    ///
    /// A package is used when a module that the `package.json` governs (the nearest one above it)
    /// imports it, whatever the subpath. `@types/x` follows `x`, `@types/node` follows any
    /// built-in module, and the peer dependencies of a used package are used too. Packages that
    /// are only run (a CLI in a script) or loaded by a tool's configuration are not seen: list
    /// them in `ignore`. A `package.json` that governs no module of the graph is skipped.
    ///
    /// ### Options
    ///
    /// `production`: do not check `devDependencies` and ignore imports from tests (`test` adds
    /// globs to `*.test.*`, `*.spec.*` and test directories); `oxcruise --production` turns it on.
    /// `sections`: which sections to check. `ignore`: globs of package names to skip.
    /// `conventions`: packages that the tools listed in the same `package.json` load by name
    /// (`@vitest/*` next to `vitest`, `*-loader` next to `webpack`) count as used; `false`
    /// turns that off and a list of names keeps only those conventions.
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

    fn requirements(&self) -> Analyses {
        Analyses::MANIFESTS
    }

    fn check_graph(&self, cx: &RuleCtx<'_>) {
        let g = cx.graph();
        let manifests = cx.manifests();
        let mut used: Vec<FxHashSet<&str>> = vec![FxHashSet::default(); manifests.len()];
        let mut owner: Vec<Option<ModuleId>> = vec![None; manifests.len()];
        for m in g.module_ids() {
            let module = g.module(m);
            if module.kind != ModuleKind::Source {
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
            for d in &manifest.deps {
                if uses
                    .as_ref()
                    .is_some_and(|u| u.is_match(&d.name, &Captures::default()))
                {
                    continue;
                }
                if !self.sections.contains(&d.section) || live.contains(d.name.as_str()) {
                    continue;
                }
                if types_target(&d.name).is_some_and(|t| live.contains(t.as_str())) {
                    continue;
                }
                if let Some(ignore) = &self.ignore
                    && ignore.is_match(&d.name, &Captures::default())
                {
                    continue;
                }
                // delete the member with the comma that separates it from a neighbour
                let section: Vec<_> = manifest
                    .deps
                    .iter()
                    .filter(|o| o.section == d.section)
                    .collect();
                let at = section.iter().position(|o| o.name == d.name).unwrap_or(0);
                let range = match (section.get(at + 1), at.checked_sub(1).map(|p| section[p])) {
                    (Some(next), _) => Some((d.range.0, next.range.0)),
                    (None, Some(prev)) => Some((prev.range.1, d.range.1)),
                    (None, None) => Some(d.range),
                };
                let fix = range.filter(|r| r.1 > r.0).map(|(s, e)| Fix {
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
