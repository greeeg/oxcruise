use std::sync::OnceLock;

use oxcruise_core::{ModuleId, ModuleKind};
use oxcruise_graph::analyses::{BitSet, reach_from};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use super::is_test_module;
use crate::conventions::ConventionFilter;
use crate::meta::{Analyses, Hooks, Rule, RuleCtx, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

/// Files that are legitimately not imported: dotfiles, declarations, and tool configuration.
const DEFAULT_IGNORE: &[&str] = &["**/.*", "**/*.d.ts", "**/*.config.*"];

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NoUnusedFilesConfig {
    /// Globs of entry points, on top of the ones oxcruise finds (`package.json` entry files, the
    /// conventions of the tools you use, `index`, `main` and `cli` files, plugins).
    pub entries: Vec<String>,
    /// Globs of modules that are never reported, on top of the defaults
    /// (`**/.*`, `**/*.d.ts`, `**/*.config.*`).
    pub ignore: Vec<String>,
    /// Which tool conventions count as entry points: `true` (all, the default), `false`, or names.
    pub conventions: ConventionFilter,
    /// Production mode: tests are not entry points and are not reported, so a module that only
    /// tests use is reported.
    pub production: bool,
    /// Globs of test modules, on top of `*.test.*`, `*.spec.*` and test directories.
    pub test: Vec<String>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Reports source modules that cannot be reached from any entry point.
    ///
    /// ### Why is this bad?
    ///
    /// A module that nothing leads to is dead code. Unlike `no-orphans`, this also finds
    /// groups of modules that import each other but are used by nothing else.
    ///
    /// ### Entry points
    ///
    /// The files `package.json` publishes (`main`, `module`, `types`, `bin`, `exports`, mapped
    /// from the build output back to source), `index`, `main` and `cli` files, the files that the
    /// tools listed in `package.json` load (tests, routes, configs; see the conventions table), the
    /// entries of plugins and the `entries` option. When none is found, nothing is reported.
    /// Mark a module with `// oxcruise-ignore-file` to keep it and what it imports.
    ///
    /// ### Options
    ///
    /// `production`: tests are neither entry points nor reported (`test` adds globs to `*.test.*`,
    /// `*.spec.*` and test directories); `oxcruise --production` turns it on.
    /// `entries`: more entry globs. `ignore`: globs of modules to skip, on top of dotfiles,
    /// declaration files and `*.config.*`. `conventions`: `false` or a list of names to use only
    /// some tool conventions.
    NoUnusedFiles, category = nursery, version = "0.1.0", config = NoUnusedFilesConfig
}

#[derive(Debug, Default)]
pub struct NoUnusedFiles {
    entries: Option<PathPatterns>,
    ignore: Option<PathPatterns>,
    conventions: ConventionFilter,
    /// Test modules, in production mode.
    test: Option<PathPatterns>,
    /// The modules that can be reached, from the first module checked.
    reached: OnceLock<Option<BitSet>>,
}

impl NoUnusedFiles {
    fn is_test(&self, path: &str) -> bool {
        self.test.as_ref().is_some_and(|t| is_test_module(path, t))
    }

    fn reached(&self, cx: &RuleCtx<'_>) -> Option<&BitSet> {
        self.reached
            .get_or_init(|| {
                let g = cx.graph();
                let mut starts: Vec<ModuleId> = cx
                    .entries()
                    .declared
                    .iter()
                    .map(|m| ModuleId(m as u32))
                    .filter(|&m| !self.is_test(&g.module(m).path))
                    .collect();
                if let Some(extra) = &self.entries {
                    let none = Captures::default();
                    starts.extend(g.module_ids().filter(|&m| {
                        let module = g.module(m);
                        module.kind == ModuleKind::Local && extra.is_match(&module.path, &none)
                    }));
                }
                if starts.is_empty() {
                    cx.warn(
                        "no-unused-files: no entry point found (package.json entries, conventions, \
                         `entries` option); nothing is reported",
                    );
                    return None;
                }
                Some(reach_from(g, &starts, &|_| true).reached)
            })
            .as_ref()
    }
}

impl Rule for NoUnusedFiles {
    const HOOKS: Hooks = Hooks::MODULE;

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: NoUnusedFilesConfig = parse_options("no-unused-files", value)?;
        c.conventions.validate()?;
        let ignore: Vec<String> = DEFAULT_IGNORE
            .iter()
            .map(|g| g.to_string())
            .chain(c.ignore)
            .collect();
        Ok(Self {
            entries: Some(PathPatterns::new(&c.entries, Role::Referencing)?),
            ignore: Some(PathPatterns::new(&ignore, Role::Referencing)?),
            conventions: c.conventions,
            test: c
                .production
                .then(|| PathPatterns::new(&c.test, Role::Referencing))
                .transpose()?,
            reached: OnceLock::new(),
        })
    }

    fn has_production_option() -> bool {
        true
    }

    fn requirements(&self) -> Analyses {
        Analyses::ENTRIES | Analyses::MANIFESTS
    }

    fn conventions(&self) -> ConventionFilter {
        self.conventions.clone()
    }

    fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        let module = cx.module(m);
        if module.kind != ModuleKind::Local || module.not_followed {
            return;
        }
        let Some(reached) = self.reached(cx) else {
            return;
        };
        if reached.contains(m.idx()) || self.is_test(&module.path) {
            return;
        }
        if let Some(ignore) = &self.ignore
            && ignore.is_match(&module.path, &Captures::default())
        {
            return;
        }
        cx.report_module(m, "this module cannot be reached from any entry point");
    }
}
