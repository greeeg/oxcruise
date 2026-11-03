use std::sync::OnceLock;

use oxcruise_core::{ExportKind, ModuleId, ModuleKind};
use oxcruise_graph::analyses::{BitSet, ExportUsage, export_usage};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use super::is_test_module;
use crate::conventions::ConventionFilter;
use crate::meta::{Analyses, Hooks, Rule, RuleCtx, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct NoUnusedExportsConfig {
    /// Globs of modules whose exports are public API, on top of the entry points oxcruise finds:
    /// modules nothing imports, and the files `package.json` points to.
    pub entries: Vec<String>,
    /// Also report unused exports of entry points, checking them against their importers only.
    pub include_entry_exports: bool,
    /// Do not report an export that the file itself uses.
    pub ignore_exports_used_in_file: bool,
    /// Report unused types and interfaces too (default `true`).
    pub types: bool,
    /// Report unused members of exported enums and namespaces (default `true`).
    pub members: bool,
    /// Globs of modules that are never reported.
    pub ignore: Vec<String>,
    /// Which tool conventions count as entry points: `true` (all, the default), `false`, or names.
    pub conventions: ConventionFilter,
    /// Production mode: tests neither are entry points nor use anything, so an export used only
    /// by tests is reported.
    pub production: bool,
    /// Globs of test modules, on top of `*.test.*`, `*.spec.*` and test directories.
    pub test: Vec<String>,
}

impl Default for NoUnusedExportsConfig {
    fn default() -> Self {
        Self {
            entries: vec![],
            include_entry_exports: false,
            ignore_exports_used_in_file: false,
            types: true,
            members: true,
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
    /// Reports exports that no other module imports, members of enums and namespaces that nothing
    /// reads, and bindings exported under two names.
    ///
    /// ### Why is this bad?
    ///
    /// An unused export is dead code, and it keeps what it uses alive. It also hides which
    /// parts of a module are really its interface.
    ///
    /// ### How it works
    ///
    /// Every static import marks the names it uses (`import { a }`, `ns.a` of a namespace
    /// import, `E.X` of an enum), and the marks travel through `export { a } from` and
    /// `export *` to the module that defines the name. Anything used some other way (a
    /// `require`, a dynamic `import()`, a namespace passed around as a value) counts as using
    /// everything. The exports of entry points are public API and are not reported: modules that
    /// no other module imports, `package.json` entry points, the files that the tools you use load
    /// by convention, and the `entries` option.
    ///
    /// ### Options
    ///
    /// `production`: leave tests out (`test` adds globs to `*.test.*`, `*.spec.*` and test
    /// directories), so an export that only a test uses is reported; `oxcruise --production`
    /// turns it on. `entries`: extra globs for public modules. `conventions`: `false` or a list of names to use
    /// only some tool conventions. `includeEntryExports`: check entry points too.
    /// `ignoreExportsUsedInFile`: skip exports that their own file uses. `types`, `members`:
    /// turn off the report of unused types and unused enum or namespace members. `ignore`:
    /// globs of modules to skip.
    ///
    /// Suppress one export with `// oxcruise-ignore-next-line no-unused-exports` above it.
    NoUnusedExports, category = nursery, version = "0.1.0", config = NoUnusedExportsConfig
}

#[derive(Debug, Default)]
pub struct NoUnusedExports {
    config: NoUnusedExportsConfig,
    entries: Option<PathPatterns>,
    ignore: Option<PathPatterns>,
    test: Option<PathPatterns>,
    /// Computed by the first module checked when the options change what is used.
    usage: OnceLock<ExportUsage>,
}

impl NoUnusedExports {
    fn is_test(&self, path: &str) -> bool {
        self.config.production && self.test.as_ref().is_some_and(|t| is_test_module(path, t))
    }

    /// The options that decide who is public and who counts as a user are the defaults: what
    /// is used is then the run's shared answer.
    fn is_plain(&self) -> bool {
        let c = &self.config;
        c.entries.is_empty()
            && !c.production
            && !c.include_entry_exports
            && c.conventions == ConventionFilter::default()
    }

    fn usage<'a>(&'a self, cx: &RuleCtx<'a>) -> &'a ExportUsage {
        if self.is_plain()
            && let Some(shared) = &cx.analyses().usage
        {
            return shared;
        }
        self.usage.get_or_init(|| {
            let g = cx.graph();
            let mut entries = cx.entries().all();
            if self.config.production {
                for m in g.module_ids().filter(|&m| self.is_test(&g.module(m).path)) {
                    entries.remove(m.idx());
                }
            }
            if let Some(extra) = &self.entries {
                let none = Captures::default();
                for m in g.module_ids() {
                    let module = g.module(m);
                    if module.kind == ModuleKind::Local && extra.is_match(&module.path, &none) {
                        entries.insert(m.idx());
                    }
                }
            }
            let seeds = if self.config.include_entry_exports {
                BitSet::new(g.module_count())
            } else {
                entries
            };
            export_usage(g, &seeds, &|m| self.is_test(&g.module(m).path))
        })
    }
}

impl Rule for NoUnusedExports {
    const HOOKS: Hooks = Hooks::MODULE;

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let config: NoUnusedExportsConfig = parse_options("no-unused-exports", value)?;
        config.conventions.validate()?;
        Ok(Self {
            entries: Some(PathPatterns::new(&config.entries, Role::Referencing)?),
            ignore: Some(PathPatterns::new(&config.ignore, Role::Referencing)?),
            test: Some(PathPatterns::new(&config.test, Role::Referencing)?),
            config,
            usage: OnceLock::new(),
        })
    }

    fn has_production_option() -> bool {
        true
    }

    fn requirements(&self) -> Analyses {
        Analyses::ENTRIES | Analyses::SYMBOLS | Analyses::MANIFESTS
    }

    fn conventions(&self) -> ConventionFilter {
        self.config.conventions.clone()
    }

    fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        let module = cx.module(m);
        if module.kind != ModuleKind::Local || module.exports.is_empty() {
            return;
        }
        if let Some(ignore) = &self.ignore
            && ignore.is_match(&module.path, &Captures::default())
        {
            return;
        }
        if self.is_test(&module.path) {
            return;
        }
        let usage = self.usage(cx);
        for (i, x) in module.exports.iter().enumerate() {
            if x.kind == ExportKind::Star {
                continue;
            }
            if let Some(first) = &x.duplicate_of {
                cx.report_export(
                    m,
                    i,
                    None,
                    format!("`{first}` and `{}` export the same binding", x.name),
                    false,
                );
            }
            if !usage.is_used(m, &x.name) {
                if (x.is_type && !self.config.types)
                    || (x.used_in_file && self.config.ignore_exports_used_in_file)
                {
                    continue;
                }
                let what = match (x.kind, x.is_type) {
                    (ExportKind::Namespace, _) => "namespace export",
                    (_, true) if x.name == "default" => "default type export",
                    (_, true) => "type export",
                    _ if x.name == "default" => "default export",
                    _ => "export",
                };
                let message = if x.name == "default" {
                    format!("unused {what}")
                } else {
                    format!("unused {what} `{}`", x.name)
                };
                cx.report_export(m, i, None, message, true);
            } else if self.config.members {
                for member in &x.members {
                    if !usage.is_member_used(m, &x.name, &member.name) {
                        cx.report_export(
                            m,
                            i,
                            Some(member.loc),
                            format!("unused member `{}.{}`", x.name, member.name),
                            false,
                        );
                    }
                }
            }
        }
    }
}
