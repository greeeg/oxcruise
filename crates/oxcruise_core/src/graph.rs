use serde::{Deserialize, Serialize};

use crate::{DepKinds, Loc, ModuleSystem};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ModuleId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EdgeId(pub u32);

impl ModuleId {
    pub fn idx(self) -> usize {
        self.0 as usize
    }
}
impl EdgeId {
    pub fn idx(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleKind {
    /// A file in the project that was parsed.
    Source,
    /// A resolved file outside the followed set (e.g. in node_modules); not parsed.
    Stub,
    /// A node:/builtin module.
    Core,
    /// A specifier that could not be resolved.
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Module {
    /// Posix path relative to the cruise root; for non-source modules the
    /// specifier or package name.
    pub path: String,
    pub kind: ModuleKind,
    /// Index into the workspace package table, if any.
    pub package: Option<u32>,
    /// Modules matched by `doNotFollow`/depth limit: present but not expanded.
    pub not_followed: bool,
    pub parse_error: Option<String>,
    /// Rules silenced for the whole file by `oxcruise-ignore-file`; `"*"` means all.
    pub suppressed: Vec<String>,
}

impl Module {
    pub fn new(path: impl Into<String>, kind: ModuleKind) -> Self {
        Self {
            path: path.into(),
            kind,
            package: None,
            not_followed: false,
            parse_error: None,
            suppressed: vec![],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub from: ModuleId,
    pub to: ModuleId,
    pub specifier: String,
    pub kinds: DepKinds,
    pub module_system: ModuleSystem,
    pub loc: Option<Loc>,
    /// Rule names suppressed on this edge by an inline comment.
    pub suppressed: Vec<String>,
}

impl Edge {
    pub fn is_type_only(&self) -> bool {
        self.kinds.contains(DepKinds::TYPE_ONLY)
    }
}

/// Immutable module graph with forward and reverse CSR adjacency.
///
/// Modules are sorted by path and edges by `(from, to, specifier)`, so the
/// frozen graph is identical regardless of how it was discovered.
#[derive(Debug, Clone)]
pub struct Graph {
    modules: Vec<Module>,
    edges: Vec<Edge>,
    /// `out_off[m]..out_off[m+1]` is the range of `edges` leaving `m`.
    out_off: Vec<u32>,
    in_off: Vec<u32>,
    in_edges: Vec<EdgeId>,
}

impl Graph {
    /// Build a graph from unordered parts. `edges` reference `modules` by the
    /// index they have in the input; this function sorts both and remaps.
    pub fn build(modules: Vec<Module>, edges: Vec<Edge>) -> Graph {
        let n = modules.len();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| modules[a].path.cmp(&modules[b].path));
        let mut remap = vec![0u32; n];
        for (new, &old) in order.iter().enumerate() {
            remap[old] = new as u32;
        }
        let mut slots: Vec<Option<Module>> = modules.into_iter().map(Some).collect();
        let modules: Vec<Module> = order
            .iter()
            .map(|&old| slots[old].take().expect("unique"))
            .collect();

        let mut edges = edges;
        for e in &mut edges {
            e.from = ModuleId(remap[e.from.idx()]);
            e.to = ModuleId(remap[e.to.idx()]);
        }
        edges.sort_by(|a, b| (a.from, a.to, &a.specifier).cmp(&(b.from, b.to, &b.specifier)));

        let mut out_off = vec![0u32; n + 1];
        let mut in_count = vec![0u32; n + 1];
        for e in &edges {
            out_off[e.from.idx() + 1] += 1;
            in_count[e.to.idx() + 1] += 1;
        }
        for i in 0..n {
            out_off[i + 1] += out_off[i];
            in_count[i + 1] += in_count[i];
        }
        let in_off = in_count.clone();
        let mut cursor = in_count;
        let mut in_edges = vec![EdgeId(0); edges.len()];
        for (i, e) in edges.iter().enumerate() {
            let slot = &mut cursor[e.to.idx()];
            in_edges[*slot as usize] = EdgeId(i as u32);
            *slot += 1;
        }
        Graph {
            modules,
            edges,
            out_off,
            in_off,
            in_edges,
        }
    }

    pub fn module_count(&self) -> usize {
        self.modules.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    pub fn modules(&self) -> &[Module] {
        &self.modules
    }
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }
    pub fn module(&self, id: ModuleId) -> &Module {
        &self.modules[id.idx()]
    }
    pub fn edge(&self, id: EdgeId) -> &Edge {
        &self.edges[id.idx()]
    }
    pub fn module_ids(&self) -> impl Iterator<Item = ModuleId> + use<> {
        (0..self.modules.len() as u32).map(ModuleId)
    }

    pub fn find(&self, path: &str) -> Option<ModuleId> {
        self.modules
            .binary_search_by(|m| m.path.as_str().cmp(path))
            .ok()
            .map(|i| ModuleId(i as u32))
    }

    /// Edge ids leaving `m`, sorted by `(to, specifier)`.
    pub fn out_edge_ids(&self, m: ModuleId) -> impl ExactSizeIterator<Item = EdgeId> + use<> {
        let (s, e) = (self.out_off[m.idx()], self.out_off[m.idx() + 1]);
        (s..e).map(EdgeId)
    }
    pub fn out_edges(&self, m: ModuleId) -> &[Edge] {
        &self.edges[self.out_off[m.idx()] as usize..self.out_off[m.idx() + 1] as usize]
    }
    pub fn in_edge_ids(&self, m: ModuleId) -> &[EdgeId] {
        &self.in_edges[self.in_off[m.idx()] as usize..self.in_off[m.idx() + 1] as usize]
    }
    pub fn out_degree(&self, m: ModuleId) -> usize {
        (self.out_off[m.idx() + 1] - self.out_off[m.idx()]) as usize
    }
    pub fn in_degree(&self, m: ModuleId) -> usize {
        (self.in_off[m.idx() + 1] - self.in_off[m.idx()]) as usize
    }
    /// Raw CSR offsets, for serialisation to the JS plugin host.
    pub fn out_offsets(&self) -> &[u32] {
        &self.out_off
    }
    pub fn in_offsets(&self) -> &[u32] {
        &self.in_off
    }
    pub fn in_edge_table(&self) -> &[EdgeId] {
        &self.in_edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(from: u32, to: u32, spec: &str) -> Edge {
        Edge {
            from: ModuleId(from),
            to: ModuleId(to),
            specifier: spec.into(),
            kinds: DepKinds::LOCAL | DepKinds::IMPORT,
            module_system: ModuleSystem::Es6,
            loc: None,
            suppressed: vec![],
        }
    }

    #[test]
    fn sorts_and_builds_csr() {
        // input order: c, a, b  -> sorted: a, b, c
        let modules = vec![
            Module::new("c.ts", ModuleKind::Source),
            Module::new("a.ts", ModuleKind::Source),
            Module::new("b.ts", ModuleKind::Source),
        ];
        let edges = vec![edge(1, 2, "./b"), edge(1, 0, "./c"), edge(2, 0, "./c")];
        let g = Graph::build(modules, edges);
        let a = g.find("a.ts").unwrap();
        let b = g.find("b.ts").unwrap();
        let c = g.find("c.ts").unwrap();
        assert_eq!((a.0, b.0, c.0), (0, 1, 2));
        assert_eq!(g.out_degree(a), 2);
        assert_eq!(
            g.out_edges(a).iter().map(|e| e.to).collect::<Vec<_>>(),
            vec![b, c]
        );
        assert_eq!(g.in_degree(c), 2);
        let from_c: Vec<_> = g.in_edge_ids(c).iter().map(|&e| g.edge(e).from).collect();
        assert_eq!(from_c, vec![a, b]);
        assert_eq!(g.in_degree(a), 0);
        assert_eq!(g.find("zzz"), None);
    }

    #[test]
    fn build_is_independent_of_input_order() {
        let mk = |order: [usize; 3]| {
            let names = ["a.ts", "b.ts", "c.ts"];
            let modules: Vec<_> = order
                .iter()
                .map(|&i| Module::new(names[i], ModuleKind::Source))
                .collect();
            let pos = |n: usize| order.iter().position(|&o| o == n).unwrap() as u32;
            let edges = vec![
                edge(pos(0), pos(1), "./b"),
                edge(pos(1), pos(2), "./c"),
                edge(pos(2), pos(0), "./a"),
            ];
            Graph::build(modules, edges)
        };
        let g1 = mk([0, 1, 2]);
        let g2 = mk([2, 0, 1]);
        assert_eq!(g1.edges(), g2.edges());
        assert_eq!(g1.modules(), g2.modules());
    }
}
