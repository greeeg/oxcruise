//! Parallel discovery of the module graph.
//!
//! One thread (the "graph thread") owns all bookkeeping. Rayon workers do the
//! expensive part per file: read, parse, extract, resolve. Results flow back
//! over a channel; the graph thread decides what to explore next.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use oxc_allocator::Allocator;
use std::collections::BTreeSet;
use std::sync::Arc;

use oxcruise_core::{
    DeclaredManifest, DepKinds, Edge, Export, FileTree, Graph, Module, ModuleId, ModuleKind,
    ModuleSystem, UsedNames,
};
use oxcruise_extract::{ExportFact, ImportFact, RawDependency, UsedRefs, extract};
use oxcruise_resolve::{Manifest, ResolveMode, Resolved, Resolver, Target, WorkspacePackage};
use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::options::CruiseOptions;
use crate::walk::expand_entries;

#[derive(Debug)]
pub struct CruiseOutput {
    pub graph: Graph,
    pub root: PathBuf,
    pub workspace: Vec<WorkspacePackage>,
    /// The `package.json` files that own modules of the graph, sorted by path. Empty unless
    /// `CruiseOptions::manifests` is set; `Module::manifest` indexes into it.
    pub manifests: Vec<DeclaredManifest>,
    /// Every file below the entry folders. `None` unless `CruiseOptions::file_tree` is set.
    pub file_tree: Option<FileTree>,
}

/// Discover the graph described by `opts`.
pub fn cruise(opts: &CruiseOptions) -> Result<CruiseOutput, String> {
    let expanded = expand_entries(opts)?;
    let file_tree = opts.file_tree.then(|| {
        FileTree::new(
            expanded
                .files
                .iter()
                .map(|p| oxcruise_resolve::relative_to(&opts.root, p))
                .collect(),
        )
    });
    let entries = expanded.sources;
    let resolver = Resolver::new(opts.resolve.clone());

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.threads.unwrap_or(0))
        .build()
        .map_err(|e| e.to_string())?;
    let discovered = discover(opts, &resolver, &pool, entries);
    let mut output = freeze(opts, &resolver, discovered);
    output.file_tree = file_tree;
    Ok(output)
}

struct Processed {
    deps: Vec<(RawDependency, Resolved)>,
    file_suppressions: Vec<String>,
    parse_error: Option<String>,
    /// Only with `ExtractOptions::symbols`.
    exports: Vec<ExportFact>,
    /// Names used per import specifier (only with `ExtractOptions::symbols`).
    used: FxHashMap<String, UsedNames>,
}

#[derive(PartialEq, Eq, Hash, Clone)]
enum NodeKey {
    Path(PathBuf),
    Core(String),
}

struct Node {
    key: NodeKey,
    kind: ModuleKind,
    rel: String,
    /// Smallest number of hops from an entry seen so far.
    depth: u32,
    not_followed: bool,
    spawned: bool,
    /// Resolved targets, filled when the file has been processed.
    out: Option<Vec<(usize, Option<usize>)>>,
    processed: Option<Processed>,
}

enum Work {
    Spawn(usize),
    Expand(usize),
}

struct Discovered {
    nodes: Vec<Node>,
}

thread_local! {
    static ALLOC: RefCell<Allocator> = RefCell::new(Allocator::default());
}

fn process(resolver: &Resolver, opts: &CruiseOptions, path: &Path) -> Processed {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return Processed {
                deps: vec![],
                file_suppressions: vec![],
                parse_error: Some(e.to_string()),
                exports: vec![],
                used: FxHashMap::default(),
            };
        }
    };
    let facts = ALLOC.with(|a| {
        let mut a = a.borrow_mut();
        a.reset();
        extract(path, &source, &opts.extract, &a)
    });
    let deps = facts
        .deps
        .into_iter()
        .map(|d| {
            // Node picks `require` vs `import` conditions by how the dependency is written.
            let mode = if d.module_system == ModuleSystem::Cjs {
                ResolveMode::Cjs
            } else {
                ResolveMode::Esm
            };
            let r = resolver.resolve(path, &d.specifier, mode);
            (d, r)
        })
        .collect();
    let mut file_suppressions = vec![];
    for s in &facts.file_suppressions {
        if s.rules.is_empty() {
            file_suppressions.push("*".to_string());
        } else {
            file_suppressions.extend(s.rules.iter().cloned());
        }
    }
    let (exports, used) = if opts.extract.symbols {
        (facts.exports, used_names(&facts.imports))
    } else {
        (vec![], FxHashMap::default())
    };
    Processed {
        deps,
        file_suppressions,
        parse_error: facts.parse_error,
        exports,
        used,
    }
}

/// Names each import specifier of a file uses. Specifiers that are only imported for their
/// side effects, or re-exported, are absent: nothing is used through them directly.
fn used_names(imports: &[ImportFact]) -> FxHashMap<String, UsedNames> {
    let mut acc: FxHashMap<&str, Option<BTreeSet<String>>> = FxHashMap::default();
    for i in imports {
        let slot = acc
            .entry(&i.specifier)
            .or_insert_with(|| Some(BTreeSet::new()));
        let Some(set) = slot else { continue };
        let members = match &i.refs {
            UsedRefs::Members(m) if !m.is_empty() => Some(m),
            _ => None,
        };
        match (i.imported.as_str(), members) {
            // a namespace nobody narrowed, or never mentioned: any export may be read
            ("*", None) => *slot = None,
            ("*", Some(m)) => set.extend(m.iter().cloned()),
            (name, None) => {
                set.insert(name.to_string());
            }
            (name, Some(m)) => set.extend(m.iter().map(|m| format!("{name}.{m}"))),
        }
    }
    acc.into_iter()
        .map(|(spec, set)| {
            let used = match set {
                None => UsedNames::All,
                Some(set) => {
                    UsedNames::Names(set.into_iter().map(String::into_boxed_str).collect())
                }
            };
            (spec.to_string(), used)
        })
        .collect()
}

/// Whether a dependency can only be a static ES `import`/`export` statement, the one form whose
/// used names are known.
fn is_static_es(dep: &RawDependency) -> bool {
    let other = DepKinds::SYNTAX_ANY
        .difference(DepKinds::IMPORT | DepKinds::EXPORT)
        .union(DepKinds::JSDOC)
        .union(DepKinds::JSDOC_BRACKET_IMPORT)
        .union(DepKinds::JSDOC_IMPORT_TAG);
    dep.module_system == ModuleSystem::Es6
        && dep.kinds.intersects(DepKinds::IMPORT | DepKinds::EXPORT)
        && !dep.kinds.intersects(other)
}

/// The graph is built on the calling thread, which sleeps while the pool parses; only the
/// parse-and-resolve jobs run on the pool.
fn discover(
    opts: &CruiseOptions,
    resolver: &Resolver,
    pool: &rayon::ThreadPool,
    entries: Vec<PathBuf>,
) -> Discovered {
    let mut g = Discoverer {
        opts,
        resolver,
        nodes: vec![],
        index: FxHashMap::default(),
        paths: FxHashMap::default(),
        pending: 0,
    };
    let (tx, rx) = mpsc::channel::<(usize, std::thread::Result<Processed>)>();

    let g = pool.in_place_scope(move |scope| {
        let mut queue: Vec<Work> = vec![];
        for e in entries {
            let rel = resolver.relative(&e);
            let not_followed = opts.do_not_follow.is_match(&rel);
            let idx = g.node(NodeKey::Path(e), ModuleKind::Local, rel);
            // doNotFollow also holds for entries: they are kept, but not explored.
            g.nodes[idx].not_followed = not_followed;
            g.reach(idx, 0, &mut queue);
        }
        let drain = |g: &mut Discoverer, q: &mut Vec<Work>| {
            while let Some(w) = q.pop() {
                match w {
                    Work::Expand(idx) => g.expand(idx, q),
                    Work::Spawn(idx) => {
                        let NodeKey::Path(path) = g.nodes[idx].key.clone() else {
                            continue;
                        };
                        g.pending += 1;
                        let tx = tx.clone();
                        scope.spawn(move |_| {
                            let done =
                                catch_unwind(AssertUnwindSafe(|| process(resolver, opts, &path)));
                            let _ = tx.send((idx, done));
                        });
                    }
                }
            }
        };
        drain(&mut g, &mut queue);

        while g.pending > 0 {
            let Ok((idx, done)) = rx.recv() else {
                break;
            };
            g.pending -= 1;
            // a panic in a worker is re-raised here rather than leaving us waiting for it
            let processed = done.unwrap_or_else(|payload| resume_unwind(payload));
            g.nodes[idx].processed = Some(processed);
            queue.push(Work::Expand(idx));
            drain(&mut g, &mut queue);
        }
        g
    });
    Discovered { nodes: g.nodes }
}

struct Discoverer<'a> {
    opts: &'a CruiseOptions,
    resolver: &'a Resolver,
    nodes: Vec<Node>,
    index: FxHashMap<NodeKey, usize>,
    /// Files seen as dependency targets, so a repeated one costs a lookup and nothing else;
    /// `None` is a file the `ignore` patterns drop.
    paths: FxHashMap<PathBuf, Option<usize>>,
    pending: usize,
}

impl Discoverer<'_> {
    fn node(&mut self, key: NodeKey, kind: ModuleKind, rel: String) -> usize {
        if let Some(&i) = self.index.get(&key) {
            return i;
        }
        let i = self.nodes.len();
        self.index.insert(key.clone(), i);
        self.nodes.push(Node {
            key,
            kind,
            rel,
            depth: u32::MAX,
            not_followed: false,
            spawned: false,
            out: None,
            processed: None,
        });
        i
    }

    fn wants_exploring(&self, idx: usize, depth: u32) -> bool {
        let n = &self.nodes[idx];
        n.kind == ModuleKind::Local
            && !n.not_followed
            && self.opts.max_depth.is_none_or(|m| depth < m)
    }

    /// Record that `idx` is reachable in `depth` hops. Queues parsing or
    /// expansion when this is news (first visit, or a shorter path to a node
    /// that was previously cut off by `max_depth`).
    fn reach(&mut self, idx: usize, depth: u32, queue: &mut Vec<Work>) {
        if self.nodes[idx].depth <= depth {
            return;
        }
        self.nodes[idx].depth = depth;
        if !self.wants_exploring(idx, depth) {
            return;
        }
        if self.nodes[idx].processed.is_some() {
            queue.push(Work::Expand(idx));
        } else if !self.nodes[idx].spawned {
            self.nodes[idx].spawned = true;
            queue.push(Work::Spawn(idx));
        }
    }

    /// Turn a processed file's resolved dependencies into nodes, and follow them.
    fn expand(&mut self, idx: usize, queue: &mut Vec<Work>) {
        if self.nodes[idx].out.is_none() {
            // taken out so `target_node` can borrow `self`
            let processed = self.nodes[idx]
                .processed
                .take()
                .expect("expand needs a processed file");
            let out = processed
                .deps
                .iter()
                .enumerate()
                .map(|(i, (_, r))| (i, self.target_node(r)))
                .collect();
            self.nodes[idx].processed = Some(processed);
            self.nodes[idx].out = Some(out);
        }
        let depth = self.nodes[idx].depth;
        if !self.wants_exploring(idx, depth) {
            return;
        }
        let targets: Vec<usize> = self.nodes[idx]
            .out
            .as_ref()
            .unwrap()
            .iter()
            .filter_map(|(_, t)| *t)
            .collect();
        for t in targets {
            self.reach(t, depth + 1, queue);
        }
    }

    /// Node for a resolved dependency, or `None` if the filters drop it.
    fn target_node(&mut self, r: &Resolved) -> Option<usize> {
        let opts = self.opts;
        let keep = |rel: &str| !opts.ignore.is_match(rel);
        match &r.target {
            Target::Skip => None,
            Target::Core(name) => keep(name).then(|| {
                self.node(
                    NodeKey::Core(name.clone()),
                    ModuleKind::Builtin,
                    name.clone(),
                )
            }),
            // Unresolved specifiers become leaf modules in `freeze`.
            Target::Unresolved(_) => None,
            Target::Local(p) => {
                if let Some(&known) = self.paths.get(p.as_path()) {
                    return known;
                }
                let rel = self.resolver.relative(p);
                if !keep(&rel) {
                    self.paths.insert(p.clone(), None);
                    return None;
                }
                let not_followed = opts.do_not_follow.is_match(&rel);
                let i = self.node(NodeKey::Path(p.clone()), ModuleKind::Local, rel);
                if not_followed {
                    self.nodes[i].not_followed = true;
                }
                self.paths.insert(p.clone(), Some(i));
                Some(i)
            }
            Target::Npm { path, .. } => {
                if let Some(&known) = self.paths.get(path.as_path()) {
                    return known;
                }
                let rel = self.resolver.relative(path);
                if !keep(&rel) {
                    self.paths.insert(path.clone(), None);
                    return None;
                }
                // node_modules are always leaves
                let i = self.node(NodeKey::Path(path.clone()), ModuleKind::Package, rel);
                self.paths.insert(path.clone(), Some(i));
                Some(i)
            }
        }
    }
}

fn freeze(opts: &CruiseOptions, resolver: &Resolver, mut d: Discovered) -> CruiseOutput {
    let ws = resolver.workspace();
    let mut modules: Vec<Module> = vec![];
    let mut spec_nodes: FxHashMap<String, u32> = FxHashMap::default();
    // `package_of` scans every workspace package, and a folder's files share the answer
    let mut package_of_dir: FxHashMap<&Path, Option<u32>> = FxHashMap::default();
    for n in &mut d.nodes {
        let mut m = Module::new(std::mem::take(&mut n.rel), n.kind);
        if let NodeKey::Path(p) = &n.key {
            m.package = match p.parent() {
                Some(dir) => *package_of_dir
                    .entry(dir)
                    .or_insert_with(|| ws.package_of(dir).map(|i| i as u32)),
                None => ws.package_of(p).map(|i| i as u32),
            };
        }
        m.not_followed = n.not_followed || (n.kind == ModuleKind::Local && n.processed.is_none());
        if let Some(p) = &mut n.processed {
            m.parse_error = p.parse_error.clone();
            m.suppressed = std::mem::take(&mut p.file_suppressions);
        }
        modules.push(m);
    }

    let manifests = if opts.manifests {
        attach_manifests(opts, resolver, &d, &mut modules)
    } else {
        vec![]
    };

    let mut edges: Vec<Edge> = vec![];
    for (from, n) in d.nodes.iter_mut().enumerate() {
        let (Some(p), Some(out)) = (&mut n.processed, &n.out) else {
            continue;
        };
        // where each specifier of this file leads, for its re-exports
        let mut leads_to: FxHashMap<&str, u32> = FxHashMap::default();
        for (i, target) in out {
            let (dep, res) = &p.deps[*i];
            let to = match (&res.target, target) {
                (Target::Unresolved(_), _) => {
                    // Unresolved specifiers become leaf modules named after the specifier.
                    if opts.ignore.is_match(&dep.specifier) {
                        continue;
                    }
                    *spec_nodes.entry(dep.specifier.clone()).or_insert_with(|| {
                        modules.push(Module::new(dep.specifier.clone(), ModuleKind::Unresolved));
                        (modules.len() - 1) as u32
                    })
                }
                (_, Some(t)) => *t as u32,
                (_, None) => continue,
            };
            let mut kinds = dep.kinds | res.kinds;
            if matches!(res.target, Target::Unresolved(_)) {
                kinds |= DepKinds::UNKNOWN;
            }
            note_target(&mut leads_to, dep, to);
            let used = if !opts.extract.symbols || !is_static_es(dep) {
                UsedNames::All
            } else {
                p.used
                    .get(&dep.specifier)
                    .cloned()
                    .unwrap_or_else(UsedNames::none)
            };
            edges.push(Edge {
                from: ModuleId(from as u32),
                to: ModuleId(to),
                specifier: dep.specifier.clone(),
                kinds,
                module_system: dep.module_system,
                loc: Some(dep.loc),
                suppressed: dep.suppressed.clone(),
                used,
            });
        }
        modules[from].exports = std::mem::take(&mut p.exports)
            .into_iter()
            .map(|e| {
                let from = e
                    .from
                    .as_deref()
                    .and_then(|s| leads_to.get(s))
                    .map(|&t| ModuleId(t));
                export_of(e, from)
            })
            .collect();
    }

    CruiseOutput {
        graph: Graph::build(modules, edges),
        root: opts.root.clone(),
        workspace: ws.packages.clone(),
        manifests,
        file_tree: None,
    }
}

/// Remember where `dep` leads, for the re-exports that read from its specifier. An ES
/// `export … from` decides: another dependency on the same specifier (a `require`) may resolve to
/// another file when the package's `exports` differ by condition.
fn note_target<'a>(leads_to: &mut FxHashMap<&'a str, u32>, dep: &'a RawDependency, to: u32) {
    if dep.kinds.contains(DepKinds::EXPORT) && dep.module_system == ModuleSystem::Es6 {
        leads_to.insert(&dep.specifier, to);
    } else {
        leads_to.entry(&dep.specifier).or_insert(to);
    }
}

/// The graph's export for a fact, with the module a re-export reads from.
fn export_of(e: ExportFact, from: Option<ModuleId>) -> Export {
    Export {
        name: e.name,
        kind: e.kind,
        is_type: e.is_type,
        from,
        imported: e.imported,
        loc: e.loc.unwrap_or_default(),
        suppressed: e.suppressed,
        members: e.members,
        used_in_file: e.used_in_file,
        duplicate_of: e.duplicate_of,
        fix: e.fix,
    }
}

/// Find the `package.json` of every source module, record the index on the module and return the
/// declarations, sorted by path so the result does not depend on discovery order.
fn attach_manifests(
    opts: &CruiseOptions,
    resolver: &Resolver,
    d: &Discovered,
    modules: &mut [Module],
) -> Vec<DeclaredManifest> {
    let mut by_dir: FxHashMap<PathBuf, Arc<Manifest>> = FxHashMap::default();
    let mut owner: Vec<(usize, PathBuf)> = vec![];
    for (i, n) in d.nodes.iter().enumerate() {
        let NodeKey::Path(p) = &n.key else { continue };
        if n.kind != ModuleKind::Local {
            continue;
        }
        if let Some(m) = p.parent().and_then(|dir| resolver.nearest_manifest(dir)) {
            owner.push((i, m.dir.clone()));
            by_dir.entry(m.dir.clone()).or_insert(m);
        }
    }
    let mut dirs: Vec<&PathBuf> = by_dir.keys().collect();
    dirs.sort();
    let index: FxHashMap<&PathBuf, u32> = dirs
        .iter()
        .enumerate()
        .map(|(i, d)| (*d, i as u32))
        .collect();
    for (node, dir) in &owner {
        modules[*node].manifest = Some(index[dir]);
    }
    // reading and scanning each file is independent; `collect` keeps the order of `dirs`
    dirs.par_iter()
        .map(|dir| by_dir[*dir].declared(&opts.root, opts.manifest_peers))
        .collect()
}
