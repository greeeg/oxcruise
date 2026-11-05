use std::collections::BTreeSet;
use std::sync::OnceLock;

use oxcruise_config::Paths;
use oxcruise_core::{CycleStep, Edge, EdgeId, Graph, ModuleId, ModuleKind, Violation, parent_path};
use oxcruise_graph::analyses::{BitSet, Scc, components_csr, shortest_cycle, strongly_connected};
use oxcruise_macros::declare_rule;
use rustc_hash::FxHashMap;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::folders::FolderPath;
use crate::meta::{Analyses, Hooks, Rule, RuleCtx, RuleMeta, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

/// One entry of `folders`: the folders whose dependencies on each other the rule checks.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CircularFolder {
    /// Globs of the folders. `{name}` captures a path segment. A module below a matching folder
    /// belongs to it. A glob matches only the folder it names.
    pub path: Paths,
}

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct CircularDependenciesConfig {
    /// Also report cycles that exist only through type-only imports (`import type`).
    /// By default only cycles that exist at runtime are reported.
    pub include_type_only: bool,
    /// Globs of modules to leave out before looking for cycles, so no cycle goes through them.
    pub ignore: Vec<String>,
    /// How many dependencies of one strongly connected component are searched for their shortest
    /// cycle (default 100); `0` removes the limit. Several of them can share one cycle.
    pub max_cycles_per_component: Option<u32>,
    /// Entries for cycles between folders. Each entry has a `path`. A dependency from a module in
    /// one folder to a module in another folder of the same entry is a dependency between the
    /// folders. A cycle between two or more of them is a violation. The limit
    /// `maxCyclesPerComponent` applies to the dependencies between folders as well.
    pub folders: Vec<CircularFolder>,
}

const DEFAULT_MAX_CYCLES_PER_COMPONENT: u32 = 100;

declare_rule! {
    /// ### What it does
    ///
    /// Reports dependencies that are part of a circular chain of imports.
    /// The message shows the shortest cycle through each dependency.
    ///
    /// ### Why is this bad?
    ///
    /// A cycle makes modules hard to load, to test and to understand alone.
    /// A cycle can also cause `undefined` values at runtime.
    ///
    /// ### Options
    ///
    /// * `includeTypeOnly`: also count type-only imports. The default is `false`.
    ///   A type-only import does not exist at runtime, so a cycle through `import type` is not a runtime cycle.
    /// * `ignore`: globs of modules to remove from the graph before the search for cycles.
    ///   Use it for a barrel file that many paths include.
    ///   To limit the rule to some modules instead, use `overrides`.
    /// * `maxCyclesPerComponent`: the number of dependencies to search in each strongly connected component.
    ///   The default is `100`. Use `0` to search all dependencies.
    ///
    /// A strongly connected component is a group of modules that import each other.
    /// It has one cycle for each dependency, and each search reads the whole group.
    /// The rule searches only the first dependencies of a group, in graph order.
    /// It then reports one extra violation on the first module of the group.
    /// That violation gives the number of dependencies that the rule did not search.
    ///
    /// ### Cycles between folders
    ///
    /// * `folders`: a list of entries, each with a `path`. `{name}` in the `path` captures a path segment.
    ///   A glob that starts with `!` removes folders from the match.
    ///   The rule groups the modules below each matching folder.
    ///   A dependency between modules in two folders of one entry is a dependency between the folders.
    ///   A cycle between two or more folders of one entry is a violation.
    ///   The rule honours `includeTypeOnly`, `ignore` and `maxCyclesPerComponent`. It also reports the cycles between modules, as before.
    ///   To silence a folder cycle, put an `oxcruise-ignore-next-line circular-dependencies` comment above one dependency of the cycle.
    ///
    /// ```json
    /// { "includeTypeOnly": true, "folders": [{ "path": "src/features/{f}" }] }
    /// ```
    CircularDependencies, category = correctness, version = "0.1.0", config = CircularDependenciesConfig
}

/// What the rule works out once per run: the components, which dependencies get a search, and
/// which components are cut short. Done up front so the result does not depend on how the
/// parallel per-edge checks are scheduled.
#[derive(Debug)]
struct Plan {
    /// Own components, only when `ignore` removes modules before the search.
    pruned: Option<(Scc, BitSet)>,
    /// Edges whose shortest cycle is searched (self-edges are always in).
    searched: BitSet,
    /// First module of each cut-short component -> (component size, dependencies left out).
    omitted: FxHashMap<ModuleId, (u32, u32)>,
}

#[derive(Debug, Default)]
pub struct CircularDependencies {
    folders: Vec<FolderPath>,
    include_type_only: bool,
    ignore: Option<PathPatterns>,
    max_per_component: u32,
    plan: OnceLock<Plan>,
}

impl CircularDependencies {
    fn keeps(&self, edge: &Edge, removed: Option<&BitSet>) -> bool {
        (self.include_type_only || !edge.is_type_only())
            && removed.is_none_or(|r| !r.contains(edge.from.idx()) && !r.contains(edge.to.idx()))
    }

    fn prune(&self, g: &Graph, ignore: &PathPatterns) -> (Scc, BitSet) {
        let mut removed = BitSet::new(g.module_count());
        let none = Captures::default();
        for m in g.module_ids() {
            if ignore.is_match(&g.module(m).path, &none) {
                removed.insert(m.idx());
            }
        }
        let scc = strongly_connected(g, &|e: &Edge| self.keeps(e, Some(&removed)));
        (scc, removed)
    }

    fn plan<'a>(&'a self, cx: &RuleCtx<'a>) -> &'a Plan {
        self.plan.get_or_init(|| {
            let g = cx.graph();
            let pruned = self.ignore.as_ref().map(|ignore| self.prune(g, ignore));
            let (scc, removed) = match &pruned {
                Some((scc, removed)) => (scc, Some(removed)),
                None => (cx.analyses().scc(!self.include_type_only), None),
            };
            let mut searched = BitSet::new(g.edge_count());
            let mut taken: FxHashMap<u32, u32> = FxHashMap::default();
            let mut left_out: FxHashMap<u32, u32> = FxHashMap::default();
            for id in (0..g.edge_count() as u32).map(EdgeId) {
                let e = g.edge(id);
                if !self.keeps(e, removed) || !scc.is_cyclic_edge(g, id) {
                    continue;
                }
                let n = taken.entry(scc.component(e.from)).or_default();
                if e.from == e.to || self.max_per_component == 0 || *n < self.max_per_component {
                    searched.insert(id.idx());
                    *n += 1;
                } else {
                    *left_out.entry(scc.component(e.from)).or_default() += 1;
                }
            }
            let mut omitted = FxHashMap::default();
            if !left_out.is_empty() {
                // module ids follow path order, so the first one met is the first by path
                for m in g.module_ids() {
                    if let Some(&n) = left_out.get(&scc.component(m)) {
                        omitted.entry(m).or_insert((scc.component_size(m), n));
                        left_out.remove(&scc.component(m));
                    }
                }
            }
            Plan {
                pruned,
                searched,
                omitted,
            }
        })
    }
}

impl Rule for CircularDependencies {
    const HOOKS: Hooks = Hooks::MODULE.union(Hooks::EDGE).union(Hooks::GRAPH);

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: CircularDependenciesConfig = parse_options(Self::NAME, value)?;
        let mut folders = vec![];
        let mut errors = vec![];
        for (i, f) in c.folders.iter().enumerate() {
            match FolderPath::new(&f.path, &format!("folders[{i}]")) {
                Ok(p) => folders.push(p),
                Err(e) => errors.push(e),
            }
        }
        if !errors.is_empty() {
            return Err(format!(
                "invalid options for `{}`: {}",
                Self::NAME,
                errors.join("; ")
            ));
        }
        Ok(Self {
            folders,
            include_type_only: c.include_type_only,
            ignore: (!c.ignore.is_empty())
                .then(|| PathPatterns::new(&c.ignore, Role::Referencing))
                .transpose()?,
            max_per_component: c
                .max_cycles_per_component
                .unwrap_or(DEFAULT_MAX_CYCLES_PER_COMPONENT),
            plan: OnceLock::new(),
        })
    }

    fn requirements(&self) -> Analyses {
        // with `ignore`, the rule computes its own components
        match (&self.ignore, self.include_type_only) {
            (Some(_), _) => Analyses::empty(),
            (None, true) => Analyses::CYCLES,
            (None, false) => Analyses::CYCLES_RUNTIME,
        }
    }

    fn check_module(&self, m: ModuleId, cx: &RuleCtx<'_>) {
        if let Some(&(size, n)) = self.plan(cx).omitted.get(&m) {
            cx.report_module(
                m,
                format!(
                    "circular dependencies: {n} more dependencies in this group of {size} \
                     modules were not searched for their cycle \
                     (maxCyclesPerComponent: {}); fix the cycles reported first, or raise the limit",
                    self.max_per_component
                ),
            );
        }
    }

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let g = cx.graph();
        let plan = self.plan(cx);
        if !plan.searched.contains(e.idx()) {
            return;
        }
        let (scc, removed) = match &plan.pruned {
            Some((scc, removed)) => (scc, Some(removed)),
            None => (cx.analyses().scc(!self.include_type_only), None),
        };
        let keep = |edge: &Edge| self.keeps(edge, removed);
        let Some(cycle) = shortest_cycle(g, scc, e, &keep) else {
            return;
        };
        let mut names: Vec<&str> = cycle
            .iter()
            .map(|s| cx.module(s.module).path.as_str())
            .collect();
        names.push(names[0]);
        cx.report_edge_with_path(
            e,
            format!("circular dependency: {}", names.join(" → ")),
            cycle,
        );
    }

    fn check_graph(&self, cx: &RuleCtx<'_>) {
        if self.folders.is_empty() {
            return;
        }
        let removed = self.plan(cx).pruned.as_ref().map(|(_, removed)| removed);
        for folder in &self.folders {
            self.folder_cycles(cx, folder, removed);
        }
    }
}

/// The folders of one `folders` entry that hold modules, numbered in the order of the modules.
struct FolderMap<'g> {
    path: &'g FolderPath,
    ids: FxHashMap<&'g str, u32>,
    names: Vec<&'g str>,
    /// The folder of a directory (the outermost matching directory above it, or itself).
    memo: FxHashMap<&'g str, Option<u32>>,
}

impl<'g> FolderMap<'g> {
    fn folder_of_dir(&mut self, dir: &'g str) -> Option<u32> {
        if dir.is_empty() {
            return None;
        }
        if let Some(&known) = self.memo.get(dir) {
            return known;
        }
        let found = self.folder_of_dir(parent_path(dir)).or_else(|| {
            (dir.starts_with(self.path.prefix()) && self.path.capture(dir).is_some()).then(|| {
                let next = self.names.len() as u32;
                let id = *self.ids.entry(dir).or_insert(next);
                if id == next {
                    self.names.push(dir);
                }
                id
            })
        });
        self.memo.insert(dir, found);
        found
    }
}

/// The distinct dependencies between folders, in CSR form, with the first dependency between
/// modules that stands for each.
struct FolderGraph {
    offsets: Vec<u32>,
    targets: Vec<u32>,
    via: Vec<EdgeId>,
}

impl FolderGraph {
    fn edges_of(&self, f: u32) -> std::ops::Range<usize> {
        self.offsets[f as usize] as usize..self.offsets[f as usize + 1] as usize
    }

    fn via(&self, from: u32, to: u32) -> EdgeId {
        let range = self.edges_of(from);
        let at = self.targets[range.clone()]
            .binary_search(&to)
            .expect("a hop of a cycle is a dependency");
        self.via[range.start + at]
    }
}

impl CircularDependencies {
    fn folder_cycles(&self, cx: &RuleCtx<'_>, folder: &FolderPath, removed: Option<&BitSet>) {
        let g = cx.graph();
        let mut map = FolderMap {
            path: folder,
            ids: FxHashMap::default(),
            names: vec![],
            memo: FxHashMap::default(),
        };
        let folder_of: Vec<Option<u32>> = g
            .modules()
            .iter()
            .map(|m| {
                (m.kind == ModuleKind::Local)
                    .then(|| map.folder_of_dir(parent_path(&m.path)))
                    .flatten()
            })
            .collect();
        let n = map.names.len();

        let mut hops: Vec<(u32, u32, EdgeId)> = vec![];
        for id in (0..g.edge_count() as u32).map(EdgeId) {
            let e = g.edge(id);
            if !self.keeps(e, removed) {
                continue;
            }
            if let (Some(a), Some(b)) = (folder_of[e.from.idx()], folder_of[e.to.idx()])
                && a != b
            {
                hops.push((a, b, id));
            }
        }
        // the first dependency between modules (the smallest edge) stands for a pair of folders
        hops.sort_unstable();
        hops.dedup_by_key(|h| (h.0, h.1));
        let mut offsets = vec![0u32; n + 1];
        for h in &hops {
            offsets[h.0 as usize + 1] += 1;
        }
        for i in 0..n {
            offsets[i + 1] += offsets[i];
        }
        let graph = FolderGraph {
            offsets,
            targets: hops.iter().map(|h| h.1).collect(),
            via: hops.iter().map(|h| h.2).collect(),
        };
        let components = components_csr(&graph.offsets, &graph.targets);

        // One search per dependency inside a component, for its shortest cycle. The cycles are
        // kept as the folders in order, starting at the smallest, so one cycle is found once.
        let mut cycles: BTreeSet<Vec<u32>> = BTreeSet::new();
        let mut searched: FxHashMap<u32, u32> = FxHashMap::default();
        let mut left_out: FxHashMap<u32, u32> = FxHashMap::default();
        let mut prev = vec![NONE; n];
        for from in 0..n as u32 {
            let c = components.comp[from as usize];
            if components.sizes[c as usize] < 2 {
                continue;
            }
            for i in graph.edges_of(from) {
                let to = graph.targets[i];
                if components.comp[to as usize] != c {
                    continue;
                }
                let done = searched.entry(c).or_default();
                if self.max_per_component != 0 && *done >= self.max_per_component {
                    *left_out.entry(c).or_default() += 1;
                    continue;
                }
                *done += 1;
                if let Some(mut cycle) =
                    shortest_folder_path(&graph, &components.comp, to, from, &mut prev)
                {
                    // `to .. from` closes with the dependency `from -> to`
                    let last = cycle.len() - 1;
                    cycle.rotate_left(last);
                    let start = (0..cycle.len()).min_by_key(|&k| cycle[k]).unwrap_or(0);
                    cycle.rotate_left(start);
                    cycles.insert(cycle);
                }
            }
        }
        // Reported as for modules: on the first module of each cut-short group, which module ids
        // (in path order) find first.
        if !left_out.is_empty() {
            for m in g.module_ids() {
                let Some(f) = folder_of[m.idx()] else {
                    continue;
                };
                let c = components.comp[f as usize];
                if let Some(n) = left_out.remove(&c) {
                    cx.report_module(
                        m,
                        format!(
                            "circular dependencies: {n} more dependencies in this group of {} \
                             folders were not searched for their cycle \
                             (maxCyclesPerComponent: {}); fix the cycles reported first, or raise the limit",
                            components.sizes[c as usize], self.max_per_component
                        ),
                    );
                }
            }
        }

        for cycle in cycles {
            let vias: Vec<EdgeId> = (0..cycle.len())
                .map(|k| graph.via(cycle[k], cycle[(k + 1) % cycle.len()]))
                .collect();
            let path: Vec<CycleStep> = vias
                .iter()
                .map(|&e| CycleStep {
                    module: g.edge(e).from,
                    edge: Some(e),
                })
                .collect();
            let mut names: Vec<&str> = cycle.iter().map(|&f| map.names[f as usize]).collect();
            names.push(names[0]);
            let message = format!("circular dependency between folders: {}", names.join(" → "));
            // Every dependency of the cycle reports it, as for modules, so a comment on any one
            // of them silences the whole cycle. `to` is empty: a pair of folders has no one target.
            for &e in &vias {
                let edge = g.edge(e);
                cx.push(Violation {
                    rule: cx.rule.to_string(),
                    level: cx.level,
                    from: edge.from,
                    to: None,
                    edge: Some(e),
                    message: message.clone(),
                    loc: edge.loc,
                    path: path.clone(),
                    subject: oxcruise_core::Subject::Module,
                    fix: None,
                });
            }
        }
    }
}

const NONE: u32 = u32::MAX;

/// The shortest path of folders from `start` to `goal` inside one component, both included.
/// `prev` is scratch space, all `NONE` on entry and on return, so it is allocated once for all the
/// searches.
fn shortest_folder_path(
    graph: &FolderGraph,
    comp: &[u32],
    start: u32,
    goal: u32,
    prev: &mut [u32],
) -> Option<Vec<u32>> {
    // the queue keeps what it has visited, so `prev` can be cleared from it
    let mut queue = vec![start];
    prev[start as usize] = start;
    let mut head = 0;
    let mut found = false;
    while head < queue.len() {
        let v = queue[head];
        head += 1;
        if v == goal {
            found = true;
            break;
        }
        for i in graph.edges_of(v) {
            let w = graph.targets[i];
            if comp[w as usize] == comp[start as usize] && prev[w as usize] == NONE {
                prev[w as usize] = v;
                queue.push(w);
            }
        }
    }
    let path = found.then(|| {
        let mut path = vec![goal];
        let mut at = goal;
        while at != start {
            at = prev[at as usize];
            path.push(at);
        }
        path.reverse();
        path
    });
    for v in queue {
        prev[v as usize] = NONE;
    }
    path
}
