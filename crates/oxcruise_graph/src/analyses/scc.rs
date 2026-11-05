use oxcruise_core::{CycleStep, Edge, EdgeId, Graph, ModuleId};

/// Strongly connected components, computed once for the whole graph.
#[derive(Debug, Clone)]
pub struct Scc {
    comp: Vec<u32>,
    sizes: Vec<u32>,
    /// Components of size one that contain a self-edge.
    self_loops: Vec<bool>,
}

impl Scc {
    pub fn component(&self, m: ModuleId) -> u32 {
        self.comp[m.idx()]
    }

    pub fn component_size(&self, m: ModuleId) -> u32 {
        self.sizes[self.comp[m.idx()] as usize]
    }

    /// Both ends in the same component (a self-edge counts): the edge lies on a cycle.
    pub fn is_cyclic_edge(&self, g: &Graph, e: EdgeId) -> bool {
        let edge = g.edge(e);
        let c = self.comp[edge.from.idx()];
        c == self.comp[edge.to.idx()] && (self.sizes[c as usize] > 1 || self.self_loops[c as usize])
    }

    pub fn component_count(&self) -> usize {
        self.sizes.len()
    }
}

/// The strongly connected components of a directed graph: the component of each node, and the
/// size of each component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Components {
    pub comp: Vec<u32>,
    pub sizes: Vec<u32>,
}

/// Iterative Tarjan. `out(v)` lists the outgoing items of node `v`, and `to(item)` is the node an
/// item leads to, or `None` to skip the item. Both the module graph and the graph of folders run
/// through this one implementation.
fn tarjan<'a, T: 'a>(
    n: usize,
    out: impl Fn(usize) -> &'a [T],
    to: impl Fn(&T) -> Option<usize>,
) -> Components {
    const UNVISITED: u32 = u32::MAX;
    let mut index = vec![UNVISITED; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![UNVISITED; n];
    let mut sizes: Vec<u32> = vec![];
    let mut stack: Vec<u32> = vec![];
    let mut next_index = 0u32;
    // (node, position within its out list)
    let mut call: Vec<(u32, usize)> = vec![];

    for root in 0..n as u32 {
        if index[root as usize] != UNVISITED {
            continue;
        }
        call.push((root, 0));
        while let Some(&(v, pos)) = call.last() {
            let vi = v as usize;
            if pos == 0 && index[vi] == UNVISITED {
                index[vi] = next_index;
                low[vi] = next_index;
                next_index += 1;
                stack.push(v);
                on_stack[vi] = true;
            }
            let items = out(vi);
            let mut advanced = false;
            let mut p = pos;
            while p < items.len() {
                let item = &items[p];
                p += 1;
                let Some(w) = to(item) else {
                    continue;
                };
                if index[w] == UNVISITED {
                    call.last_mut().unwrap().1 = p;
                    call.push((w as u32, 0));
                    advanced = true;
                    break;
                } else if on_stack[w] {
                    low[vi] = low[vi].min(index[w]);
                }
            }
            if advanced {
                continue;
            }
            call.pop();
            if let Some(&(parent, _)) = call.last() {
                let pi = parent as usize;
                low[pi] = low[pi].min(low[vi]);
            }
            if low[vi] == index[vi] {
                let id = sizes.len() as u32;
                let mut size = 0;
                loop {
                    let w = stack.pop().expect("tarjan stack");
                    on_stack[w as usize] = false;
                    comp[w as usize] = id;
                    size += 1;
                    if w == v {
                        break;
                    }
                }
                sizes.push(size);
            }
        }
    }
    Components { comp, sizes }
}

/// The strongly connected components of a graph in CSR form: the targets of node `v` are
/// `targets[offsets[v]..offsets[v + 1]]`.
pub fn components_csr(offsets: &[u32], targets: &[u32]) -> Components {
    let n = offsets.len().saturating_sub(1);
    tarjan(
        n,
        |v| &targets[offsets[v] as usize..offsets[v + 1] as usize],
        |&t| Some(t as usize),
    )
}

/// Tarjan over the edges accepted by `keep`.
///
/// Rules such as `circular-dependencies` (by default) pass a filter that drops
/// type-only edges *before* the search, so the result is exact.
pub fn strongly_connected(g: &Graph, keep: &(dyn Fn(&Edge) -> bool + Sync)) -> Scc {
    let Components { comp, sizes } = tarjan(
        g.module_count(),
        |v| g.out_edges(ModuleId(v as u32)),
        |e: &Edge| keep(e).then(|| e.to.idx()),
    );
    let mut self_loops = vec![false; sizes.len()];
    for e in g.edges() {
        if e.from == e.to && keep(e) {
            self_loops[comp[e.from.idx()] as usize] = true;
        }
    }
    Scc {
        comp,
        sizes,
        self_loops,
    }
}

/// Reusable BFS state, so finding the shortest cycle of every edge does not allocate and clear
/// two `n`-sized vectors per edge. A slot is "set" when its stamp equals the current one.
#[derive(Default)]
struct Scratch {
    stamp: u32,
    seen: Vec<u32>,
    via: Vec<EdgeId>,
    queue: Vec<ModuleId>,
}

impl Scratch {
    fn begin(&mut self, n: usize) {
        if self.seen.len() < n {
            self.seen.resize(n, 0);
            self.via.resize(n, EdgeId(u32::MAX));
        }
        if self.stamp == u32::MAX {
            self.seen.iter_mut().for_each(|s| *s = 0);
            self.stamp = 0;
        }
        self.stamp += 1;
        self.queue.clear();
    }
}

thread_local! {
    static SCRATCH: std::cell::RefCell<Scratch> = std::cell::RefCell::new(Scratch::default());
}

/// Shortest cycle through `edge`: `edge.from -> edge.to -> ... -> edge.from`.
///
/// Steps list each module with the edge that leaves it; the last step's edge
/// returns to the first module. `None` if the edge is not on a cycle.
pub fn shortest_cycle(
    g: &Graph,
    scc: &Scc,
    edge: EdgeId,
    keep: &dyn Fn(&Edge) -> bool,
) -> Option<Vec<CycleStep>> {
    let e = g.edge(edge);
    if !keep(e) || !scc.is_cyclic_edge(g, edge) {
        return None;
    }
    let (a, b) = (e.from, e.to);
    if a == b {
        return Some(vec![CycleStep {
            module: a,
            edge: Some(edge),
        }]);
    }
    // BFS b -> a, staying inside the component.
    let comp = scc.component(a);
    SCRATCH.with(|cell| {
        let mut sc = cell.borrow_mut();
        sc.begin(g.module_count());
        let stamp = sc.stamp;
        sc.seen[b.idx()] = stamp;
        sc.queue.push(b);
        let mut head = 0;
        let mut found = false;
        'bfs: while head < sc.queue.len() {
            let m = sc.queue[head];
            head += 1;
            for id in g.out_edge_ids(m) {
                let ed = g.edge(id);
                if !keep(ed) || sc.seen[ed.to.idx()] == stamp || scc.component(ed.to) != comp {
                    continue;
                }
                sc.seen[ed.to.idx()] = stamp;
                sc.via[ed.to.idx()] = id;
                if ed.to == a {
                    found = true;
                    break 'bfs;
                }
                sc.queue.push(ed.to);
            }
        }
        if !found {
            return None;
        }
        // Walk back from a to b.
        let mut back: Vec<EdgeId> = vec![];
        let mut cur = a;
        while cur != b {
            let id = sc.via[cur.idx()];
            back.push(id);
            cur = g.edge(id).from;
        }
        back.reverse();
        let mut steps = vec![CycleStep {
            module: a,
            edge: Some(edge),
        }];
        for id in back {
            steps.push(CycleStep {
                module: g.edge(id).from,
                edge: Some(id),
            });
        }
        Some(steps)
    })
}
