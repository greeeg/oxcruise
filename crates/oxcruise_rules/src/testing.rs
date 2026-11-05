//! Graph-fixture harness for rule tests.
//!
//! Graphs are written as lines: `from -> to [as specifier] [kind kind ...]`, or a bare path for an
//! isolated module. Extra tokens in brackets: `ignore=rule` suppresses a rule on that edge, and
//! `ignore-file=rule` on a bare module line suppresses it for the file (`*` for all rules). Targets with a JS/TS extension are source modules; `core`
//! kinds make a builtin, `unknown` an unresolved specifier, anything else a package file.
//!
//! Symbols: `names=a,b` on an edge sets the names it uses (`names=` none; without it, all);
//! `reexport=x:y` (`x` read from the target, exported as `y`), `star` and `ns=name` make the
//! edge a re-export. On a module line, `exports=a,B:type,E{A+B},c~rule,d^,e@a` declares exports
//! (`:type` a type, `{..}` members, `~rule` suppressed, `^` used in its file, `@a` duplicate of `a`).

use oxcruise_config::OxcruiseConfig;
use oxcruise_core::{
    DeclaredDep, DeclaredManifest, DepKinds, DepSection, Edge, Export, ExportKind, ExportMember,
    Graph, Loc, Module, ModuleId, ModuleKind, ModuleSystem, Subject, UsedNames,
};
use oxcruise_resolve::WorkspacePackage;
use rustc_hash::FxHashMap;

use crate::engine::{Engine, RunResult};

pub struct Tester {
    config: OxcruiseConfig,
    /// `(path prefix, package index)`
    packages: Vec<(String, u32)>,
    manifests: Vec<DeclaredManifest>,
}

/// `name[:type][{A+B}][~rule][^][@duplicate]`
fn parse_export(token: &str, line: u32) -> Export {
    let end = token.find([':', '{', '~', '^', '@']).unwrap_or(token.len());
    let mut x = Export {
        name: token[..end].to_string(),
        kind: ExportKind::Local,
        is_type: false,
        from: None,
        imported: None,
        loc: Loc { line, col: 1 },
        suppressed: vec![],
        members: vec![],
        used_in_file: false,
        duplicate_of: None,
        fix: None,
    };
    let mut rest = &token[end..];
    while let Some(c) = rest.chars().next() {
        let body = &rest[1..];
        let stop = body.find([':', '{', '~', '^', '@']).unwrap_or(body.len());
        match c {
            ':' if body.starts_with("type") => x.is_type = true,
            '{' => {
                let close = body.find('}').expect("closing brace");
                x.members = body[..close]
                    .split('+')
                    .map(|m| ExportMember {
                        name: m.to_string(),
                        loc: Loc { line, col: 1 },
                    })
                    .collect();
                rest = &body[close + 1..];
                continue;
            }
            '~' => x.suppressed.push(body[..stop].to_string()),
            '^' => x.used_in_file = true,
            '@' => x.duplicate_of = Some(body[..stop].to_string()),
            other => panic!("bad export token `{token}` at `{other}`"),
        }
        rest = &body[stop..];
    }
    x
}

impl Tester {
    /// Enable one rule at `error` with the given options (`null` for none).
    pub fn rule(name: &str, options: serde_json::Value) -> Self {
        let entry = match options {
            serde_json::Value::Null => serde_json::json!("error"),
            serde_json::Value::Object(mut o) => {
                o.insert("level".into(), "error".into());
                serde_json::Value::Object(o)
            }
            other => panic!("the options of a rule are an object, not {other}"),
        };
        Self::config(serde_json::json!({ "rules": { name: entry } }))
    }

    /// A config as written by a user, except that no category is on unless the test says so, so
    /// a test sees only the rules it names.
    pub fn config(mut json: serde_json::Value) -> Self {
        if let Some(o) = json.as_object_mut() {
            o.entry("categories")
                .or_insert(serde_json::json!({ "correctness": "off", "suspicious": "off" }));
        }
        let config: OxcruiseConfig = serde_json::from_value(json).expect("valid test config");
        Self {
            config,
            packages: vec![],
            manifests: vec![],
        }
    }

    /// A `package.json` in `rel_dir` (`""` for the root) declaring `(section, name)` pairs, with
    /// section one of `dependencies`, `devDependencies`, `peerDependencies`,
    /// `optionalDependencies`. It governs the modules below `rel_dir` that no deeper one governs.
    /// Declared packages carry no peers; name `host>peer` to give `host` the peer `peer`.
    pub fn with_manifest(mut self, rel_dir: &str, deps: &[(&str, &str)]) -> Self {
        let section = |s: &str| {
            DepSection::ALL
                .into_iter()
                .find(|d| d.as_str() == s)
                .unwrap_or_else(|| panic!("unknown section `{s}`"))
        };
        let mut declared: Vec<DeclaredDep> = vec![];
        for (i, (s, name)) in deps.iter().enumerate() {
            let (name, peer) = match name.split_once('>') {
                Some((host, peer)) => (host, Some(peer)),
                None => (*name, None),
            };
            match declared.iter_mut().find(|d| d.name == name) {
                Some(d) => d.peers.extend(peer.map(str::to_string)),
                None => declared.push(DeclaredDep {
                    name: name.to_string(),
                    section: section(s),
                    loc: Loc {
                        line: i as u32 + 2,
                        col: 5,
                    },
                    range: (0, 0),
                    peers: peer.map(str::to_string).into_iter().collect(),
                }),
            }
        }
        let join = |name: &str| {
            if rel_dir.is_empty() {
                name.to_string()
            } else {
                format!("{rel_dir}/{name}")
            }
        };
        self.manifests.push(DeclaredManifest {
            rel_path: join("package.json"),
            rel_dir: rel_dir.to_string(),
            name: None,
            declared: declared.iter().map(|d| d.name.clone()).collect(),
            deps: declared,
            entry_files: vec![],
            out_dir: None,
            root_dir: None,
        });
        self
    }

    /// Publish `entry_files` from the manifest in `rel_dir` (added with `with_manifest`), as
    /// `main`/`exports` would, and tell it where the build goes (`out_dir`, `root_dir`).
    pub fn with_entry_files(
        mut self,
        rel_dir: &str,
        entry_files: &[&str],
        dirs: (Option<&str>, Option<&str>),
    ) -> Self {
        let m = self
            .manifests
            .iter_mut()
            .find(|m| m.rel_dir == rel_dir)
            .expect("manifest added first");
        m.entry_files = entry_files.iter().map(|s| s.to_string()).collect();
        m.out_dir = dirs.0.map(str::to_string);
        m.root_dir = dirs.1.map(str::to_string);
        self
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
                .filter(|t| {
                    ![
                        "ignore=",
                        "ignore-file=",
                        "names=",
                        "reexport=",
                        "ns=",
                        "exports=",
                    ]
                    .iter()
                    .any(|p| t.starts_with(p))
                        && *t != "star"
                })
                .collect();
            let Some((from, to)) = spec.split_once("->") else {
                let id = module_id(spec.trim(), ModuleKind::Local, &mut modules);
                let module = &mut modules[id as usize];
                module.suppressed.extend(
                    tokens
                        .iter()
                        .filter_map(|t| t.strip_prefix("ignore-file="))
                        .map(str::to_string),
                );
                for list in tokens.iter().filter_map(|t| t.strip_prefix("exports=")) {
                    module.exports.extend(
                        list.split(',')
                            .filter(|s| !s.is_empty())
                            .map(|e| parse_export(e, n as u32 + 1)),
                    );
                }
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
                ModuleKind::Local
            } else if kinds.contains(DepKinds::CORE) {
                ModuleKind::Builtin
            } else if kinds.contains(DepKinds::UNKNOWN) {
                ModuleKind::Unresolved
            } else {
                ModuleKind::Package
            };
            if to_kind == ModuleKind::Local {
                kinds |= DepKinds::LOCAL;
            }
            if !kinds.intersects(DepKinds::SYNTAX_ANY) {
                kinds |= DepKinds::IMPORT;
            }
            let f = module_id(from, ModuleKind::Local, &mut modules);
            let t = module_id(to, to_kind, &mut modules);
            let used = match tokens.iter().find_map(|t| t.strip_prefix("names=")) {
                Some(list) => UsedNames::Names(
                    list.split(',')
                        .filter(|s| !s.is_empty())
                        .map(Into::into)
                        .collect(),
                ),
                None => UsedNames::All,
            };
            let loc = Loc {
                line: n as u32 + 1,
                col: 1,
            };
            let reexport = |name: &str, kind, imported: Option<&str>| Export {
                name: name.to_string(),
                kind,
                is_type: false,
                from: Some(ModuleId(t)),
                imported: imported.map(str::to_string),
                loc,
                suppressed: vec![],
                members: vec![],
                used_in_file: false,
                duplicate_of: None,
                fix: None,
            };
            for tok in &tokens {
                if let Some(r) = tok.strip_prefix("reexport=") {
                    let (imported, name) = r.split_once(':').unwrap_or((r, r));
                    modules[f as usize].exports.push(reexport(
                        name,
                        ExportKind::Reexport,
                        Some(imported),
                    ));
                } else if let Some(name) = tok.strip_prefix("ns=") {
                    modules[f as usize]
                        .exports
                        .push(reexport(name, ExportKind::Namespace, None));
                } else if *tok == "star" {
                    modules[f as usize]
                        .exports
                        .push(reexport("*", ExportKind::Star, None));
                }
            }
            edges.push(Edge {
                from: ModuleId(f),
                to: ModuleId(t),
                specifier: specifier.to_string(),
                kinds,
                module_system: ModuleSystem::Es6,
                loc: Some(loc),
                suppressed,
                used,
            });
        }
        // the deepest manifest above a source module governs it
        for m in &mut modules {
            if m.kind != ModuleKind::Local {
                continue;
            }
            m.manifest = self
                .manifests
                .iter()
                .enumerate()
                .filter(|(_, d)| {
                    d.rel_dir.is_empty() || m.path.starts_with(&format!("{}/", d.rel_dir))
                })
                .max_by_key(|(_, d)| d.rel_dir.len())
                .map(|(i, _)| i as u32);
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
        engine.run(
            g,
            std::path::Path::new("/"),
            &self.workspace(),
            &self.manifests,
            has_symbols(g),
        )
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

/// One line per violation: `level rule file:line:col from -> to: message`.
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
                v.level.as_str(),
                v.rule,
                match &v.subject {
                    Subject::File(f) => f,
                    _ => &g.module(v.from).path,
                },
                loc,
                to,
                v.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Test graphs are written by hand, so ask them: a project without exports and with only `require`
/// edges reads as "no symbols".
fn has_symbols(g: &Graph) -> bool {
    g.modules().iter().any(|m| !m.exports.is_empty())
        || g.edges().iter().any(|e| e.used != UsedNames::All)
}
