//! Parallel discovery of the module graph.
//!
//! One thread (the "graph thread") owns all bookkeeping. Rayon workers do the
//! expensive part per file: read, parse, extract, resolve. Results flow back
//! over a channel; the graph thread decides what to explore next.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, TryRecvError};

use oxc_allocator::Allocator;
use oxcruise_core::{DepKinds, Edge, Graph, Module, ModuleId, ModuleKind, ModuleSystem};
use oxcruise_extract::{RawDependency, extract};
use oxcruise_resolve::{ResolveMode, Resolved, Resolver, Target, WorkspacePackage};
use rustc_hash::FxHashMap;

use crate::options::CruiseOptions;
use crate::walk::expand_entries;

#[derive(Debug)]
pub struct CruiseOutput {
    pub graph: Graph,
    pub root: PathBuf,
    pub workspace: Vec<WorkspacePackage>,
}

/// Discover the graph described by `opts`.
pub fn cruise(opts: &CruiseOptions) -> Result<CruiseOutput, String> {
    let entries = expand_entries(opts)?;
    let resolver = Resolver::new(opts.resolve.clone());

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(opts.threads.unwrap_or(0))
        .build()
        .map_err(|e| e.to_string())?;
    let discovered = pool.install(|| discover(opts, &resolver, entries));
    Ok(freeze(opts, &resolver, discovered))
}

struct Processed {
    deps: Vec<(RawDependency, Resolved)>,
    file_suppressions: Vec<String>,
    parse_error: Option<String>,
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
    Processed {
        deps,
        file_suppressions,
        parse_error: facts.parse_error,
    }
}

fn discover(opts: &CruiseOptions, resolver: &Resolver, entries: Vec<PathBuf>) -> Discovered {
    let mut g = Discoverer {
        opts,
        resolver,
        nodes: vec![],
        index: FxHashMap::default(),
        pending: 0,
    };
    let (tx, rx) = mpsc::channel::<(usize, Processed)>();

    let g = rayon::scope(move |scope| {
        let mut queue: Vec<Work> = vec![];
        for e in entries {
            let rel = resolver.relative(&e);
            let not_followed = opts.do_not_follow.is_match(&rel);
            let idx = g.node(NodeKey::Path(e), ModuleKind::Source, rel);
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
                            let _ = tx.send((idx, process(resolver, opts, &path)));
                        });
                    }
                }
            }
        };
        drain(&mut g, &mut queue);

        while g.pending > 0 {
            match rx.try_recv() {
                Ok((idx, processed)) => {
                    g.pending -= 1;
                    g.nodes[idx].processed = Some(processed);
                    queue.push(Work::Expand(idx));
                    drain(&mut g, &mut queue);
                }
                // Workers (including this one) must keep running jobs while we wait.
                Err(TryRecvError::Empty) => {
                    if rayon::yield_now() != Some(rayon::Yield::Executed) {
                        std::thread::yield_now();
                    }
                }
                Err(TryRecvError::Disconnected) => break,
            }
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
        n.kind == ModuleKind::Source
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
            let deps: Vec<_> = self.nodes[idx]
                .processed
                .as_ref()
                .expect("expand needs a processed file")
                .deps
                .iter()
                .map(|(_, r)| r.clone())
                .collect();
            let mut out = Vec::with_capacity(deps.len());
            for (i, r) in deps.iter().enumerate() {
                out.push((i, self.target_node(r)));
            }
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
            Target::Core(name) => keep(name)
                .then(|| self.node(NodeKey::Core(name.clone()), ModuleKind::Core, name.clone())),
            // Unresolved specifiers become leaf modules in `freeze`.
            Target::Unresolved(_) => None,
            Target::Local(p) => {
                let rel = self.resolver.relative(p);
                if !keep(&rel) {
                    return None;
                }
                let i = self.node(NodeKey::Path(p.clone()), ModuleKind::Source, rel.clone());
                if opts.do_not_follow.is_match(&rel) {
                    self.nodes[i].not_followed = true;
                }
                Some(i)
            }
            Target::Npm { path, .. } => {
                let rel = self.resolver.relative(path);
                if !keep(&rel) {
                    return None;
                }
                // node_modules are always leaves
                Some(self.node(NodeKey::Path(path.clone()), ModuleKind::Stub, rel))
            }
        }
    }
}

fn freeze(opts: &CruiseOptions, resolver: &Resolver, d: Discovered) -> CruiseOutput {
    let ws = resolver.workspace();
    let mut modules: Vec<Module> = vec![];
    let mut spec_nodes: FxHashMap<String, u32> = FxHashMap::default();
    for n in &d.nodes {
        let mut m = Module::new(n.rel.clone(), n.kind);
        if let NodeKey::Path(p) = &n.key {
            m.package = ws.package_of(p).map(|i| i as u32);
        }
        m.not_followed = n.not_followed || (n.kind == ModuleKind::Source && n.processed.is_none());
        if let Some(p) = &n.processed {
            m.parse_error = p.parse_error.clone();
            m.suppressed = p.file_suppressions.clone();
        }
        modules.push(m);
    }

    let mut edges: Vec<Edge> = vec![];
    for (from, n) in d.nodes.iter().enumerate() {
        let (Some(p), Some(out)) = (&n.processed, &n.out) else {
            continue;
        };
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
            edges.push(Edge {
                from: ModuleId(from as u32),
                to: ModuleId(to),
                specifier: dep.specifier.clone(),
                kinds,
                module_system: dep.module_system,
                loc: Some(dep.loc),
                suppressed: dep.suppressed.clone(),
            });
        }
    }

    CruiseOutput {
        graph: Graph::build(modules, edges),
        root: opts.root.clone(),
        workspace: ws.packages.clone(),
    }
}
