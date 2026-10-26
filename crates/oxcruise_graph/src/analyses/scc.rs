use std::collections::VecDeque;

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

/// Iterative Tarjan over the edges accepted by `keep`.
///
/// Rules such as `no-circular` with `ignoreTypeOnly` pass a filter that drops
/// type-only edges *before* the search, so the result is exact.
pub fn strongly_connected(g: &Graph, keep: &(dyn Fn(&Edge) -> bool + Sync)) -> Scc {
    let n = g.module_count();
    const UNVISITED: u32 = u32::MAX;
    let mut index = vec![UNVISITED; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![UNVISITED; n];
    let mut sizes: Vec<u32> = vec![];
    let mut stack: Vec<u32> = vec![];
    let mut next_index = 0u32;
    // (module, position within its out-edge list)
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
            let out = g.out_edges(ModuleId(v));
            let mut advanced = false;
            let mut p = pos;
            while p < out.len() {
                let e = &out[p];
                p += 1;
                if !keep(e) {
                    continue;
                }
                let w = e.to.idx();
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
    let mut pred: Vec<Option<EdgeId>> = vec![None; g.module_count()];
    let mut seen = vec![false; g.module_count()];
    let mut q = VecDeque::from([b]);
    seen[b.idx()] = true;
    while let Some(m) = q.pop_front() {
        if m == a {
            break;
        }
        for id in g.out_edge_ids(m) {
            let ed = g.edge(id);
            if !keep(ed) || seen[ed.to.idx()] || scc.component(ed.to) != comp {
                continue;
            }
            seen[ed.to.idx()] = true;
            pred[ed.to.idx()] = Some(id);
            q.push_back(ed.to);
        }
    }
    if !seen[a.idx()] {
        return None;
    }
    // Walk back from a to b.
    let mut back: Vec<EdgeId> = vec![];
    let mut cur = a;
    while cur != b {
        let id = pred[cur.idx()].expect("bfs predecessor");
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
}
