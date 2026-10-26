//! Graph-fixture harness for rule tests.
//!
//! Graphs are written as lines: `from -> to [as specifier] [kind kind ...]`, or a bare path for an
//! isolated module. Extra tokens in brackets: `ignore=rule` suppresses a rule on that edge, and
//! `ignore-file=rule` on a bare module line suppresses it for the file (`*` for all rules). Targets with a JS/TS extension are source modules; `core`
//! kinds make a builtin, `unknown` an unresolved specifier, anything else an npm stub.

use oxcruise_config::OxcruiseConfig;
use oxcruise_core::{DepKinds, Edge, Graph, Loc, Module, ModuleId, ModuleKind, ModuleSystem};
use oxcruise_resolve::WorkspacePackage;
use rustc_hash::FxHashMap;

use crate::engine::{Engine, RunResult};

pub struct Tester {
    config: OxcruiseConfig,
    /// `(path prefix, package index)`
    packages: Vec<(String, u32)>,
}

impl Tester {
    /// Enable one rule at `error` with the given options (`null` for none).
    pub fn rule(name: &str, options: serde_json::Value) -> Self {
        let entry = if options.is_null() {
            serde_json::json!("error")
        } else {
            serde_json::json!(["error", options])
        };
        Self::config(serde_json::json!({ "rules": { name: entry } }))
    }

    pub fn config(json: serde_json::Value) -> Self {
        let config: OxcruiseConfig = serde_json::from_value(json).expect("valid test config");
        Self {
            config,
            packages: vec![],
        }
    }

    /// Modules under `prefix` belong to workspace package `index` (named `pkg{index}`).
    pub fn with_package(mut self, prefix: &str, index: u32) -> Self {
        self.packages.push((prefix.to_string(), index));
        self
    }

    pub fn graph(&self, lines: &[&str]) -> Graph {
        let mut modules: Vec<Module> = vec![];
        let mut index: FxHashMap<String, u32> = FxHashMap::default();
        let mut edges = vec![];
        let is_source = |p: &str| {
            matches!(
                p.rsplit('.').next(),
                Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "mts" | "cts" | "vue")
            )
        };
        let mut module_id = |path: &str, kind: ModuleKind, modules: &mut Vec<Module>| -> u32 {
            *index.entry(path.to_string()).or_insert_with(|| {
                let mut m = Module::new(path, kind);
                m.package = self
                    .packages
                    .iter()
                    .find(|(p, _)| path.starts_with(p.as_str()))
                    .map(|(_, i)| *i);
                modules.push(m);
                (modules.len() - 1) as u32
            })
        };

        for (n, line) in lines.iter().enumerate() {
            let (spec, kinds_str) = match line.split_once('[') {
                Some((a, b)) => (a.trim(), b.trim_end().trim_end_matches(']')),
                None => (line.trim(), ""),
            };
            let tokens: Vec<&str> = kinds_str.split_whitespace().collect();
            let suppressed: Vec<String> = tokens
                .iter()
                .filter_map(|t| t.strip_prefix("ignore="))
                .map(str::to_string)
                .collect();
            let kind_names: Vec<&str> = tokens
                .iter()
                .copied()
                .filter(|t| !t.starts_with("ignore=") && !t.starts_with("ignore-file="))
                .collect();
            let Some((from, to)) = spec.split_once("->") else {
                let id = module_id(spec.trim(), ModuleKind::Source, &mut modules);
                modules[id as usize].suppressed.extend(
                    tokens
                        .iter()
                        .filter_map(|t| t.strip_prefix("ignore-file="))
                        .map(str::to_string),
                );
                continue;
            };
            let (from, to) = (from.trim(), to.trim());
            let (to, specifier) = match to.split_once(" as ") {
                Some((t, s)) => (t.trim(), s.trim()),
                None => (to, to),
            };
            let mut kinds = DepKinds::empty();
            for k in &kind_names {
                kinds |= DepKinds::from_depcruise_name(k)
                    .unwrap_or_else(|| panic!("unknown kind `{k}` in test graph"));
            }
            let to_kind = if is_source(to) {
                ModuleKind::Source
            } else if kinds.contains(DepKinds::CORE) {
                ModuleKind::Core
            } else if kinds.contains(DepKinds::UNKNOWN) {
                ModuleKind::Unresolved
            } else {
                ModuleKind::Stub
            };
            if to_kind == ModuleKind::Source {
                kinds |= DepKinds::LOCAL;
            }
            if !kinds.intersects(DepKinds::SYNTAX_ANY) {
                kinds |= DepKinds::IMPORT;
            }
            let f = module_id(from, ModuleKind::Source, &mut modules);
            let t = module_id(to, to_kind, &mut modules);
            edges.push(Edge {
                from: ModuleId(f),
                to: ModuleId(t),
                specifier: specifier.to_string(),
                kinds,
                module_system: ModuleSystem::Es6,
                loc: Some(Loc {
                    line: n as u32 + 1,
                    col: 1,
                }),
                suppressed,
            });
        }
        Graph::build(modules, edges)
    }

    pub fn workspace(&self) -> Vec<WorkspacePackage> {
        let n = self.packages.iter().map(|(_, i)| *i + 1).max().unwrap_or(0);
        (0..n)
            .map(|i| WorkspacePackage {
                name: format!("pkg{i}"),
                dir: format!("/pkg{i}").into(),
                rel_dir: format!("pkg{i}"),
            })
            .collect()
    }

    pub fn run_graph(&self, g: &Graph) -> RunResult {
        let engine =
            Engine::new(&self.config, None).unwrap_or_else(|e| panic!("bad test config: {e:?}"));
        engine.run(g, std::path::Path::new("/"), &self.workspace())
    }

    pub fn run(&self, lines: &[&str]) -> RunResult {
        self.run_graph(&self.graph(lines))
    }

    /// Assert the graph has no violations.
    pub fn pass(&self, lines: &[&str]) -> &Self {
        let g = self.graph(lines);
        let r = self.run_graph(&g);
        assert!(
            r.violations.is_empty(),
            "expected no violations, got:\n{}",
            render(&g, &r)
        );
        self
    }

    /// Assert the graph has violations and return them rendered, for snapshots.
    pub fn fail(&self, lines: &[&str]) -> String {
        let g = self.graph(lines);
        let r = self.run_graph(&g);
        assert!(!r.violations.is_empty(), "expected violations, got none");
        render(&g, &r)
    }
}

/// One line per violation: `severity rule file:line:col from -> to: message`.
pub fn render(g: &Graph, r: &RunResult) -> String {
    r.violations
        .iter()
        .map(|v| {
            let loc = v
                .loc
                .map_or(String::new(), |l| format!(":{}:{}", l.line, l.col));
            let to =
                v.to.map_or(String::new(), |t| format!(" -> {}", g.module(t).path));
            format!(
                "{} {} {}{}{}: {}",
                v.severity.as_str(),
                v.rule,
                g.module(v.from).path,
                loc,
                to,
                v.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
