use oxcruise_core::{CycleStep, Edge, EdgeId, Graph, ModuleId};

use super::BitSet;

/// Everything reachable from a set of start modules, with the edge that
/// first reached each module (so a path can be reported).
#[derive(Debug, Clone)]
pub struct Reach {
    pub reached: BitSet,
    pred: Vec<Option<EdgeId>>,
    /// Modules reached by the last search, in visiting order.
    visited: Vec<ModuleId>,
}

impl Reach {
    pub fn contains(&self, m: ModuleId) -> bool {
        self.reached.contains(m.idx())
    }

    /// Path from a start module to `target`, or `None` if unreachable (or `target` is a start).
    pub fn path_to(&self, g: &Graph, target: ModuleId) -> Option<Vec<CycleStep>> {
        if !self.contains(target) {
            return None;
        }
        let mut edges = vec![];
        let mut cur = target;
        while let Some(id) = self.pred[cur.idx()] {
            edges.push(id);
            cur = g.edge(id).from;
        }
        edges.reverse();
        let mut steps: Vec<CycleStep> = edges
            .iter()
            .map(|&id| CycleStep {
                module: g.edge(id).from,
                edge: Some(id),
            })
            .collect();
        if steps.is_empty() {
            return None;
        }
        steps.push(CycleStep {
            module: target,
            edge: None,
        });
        Some(steps)
    }
}

/// Breadth-first search over edges accepted by `keep`. Start modules are reached at depth zero.
pub fn reach_from(g: &Graph, starts: &[ModuleId], keep: &dyn Fn(&Edge) -> bool) -> Reach {
    let mut reach = Reach::new(g.module_count());
    reach.run(g, starts, keep);
    reach
}

impl Reach {
    /// An empty result for a graph of `n` modules, to be filled by [`Reach::run`].
    pub fn new(n: usize) -> Self {
        Self {
            reached: BitSet::new(n),
            pred: vec![None; n],
            visited: vec![],
        }
    }

    /// Search again into the same buffers. Only what the previous search touched is cleared, so
    /// many searches from different starts cost what they visit, not the size of the graph.
    pub fn run(&mut self, g: &Graph, starts: &[ModuleId], keep: &dyn Fn(&Edge) -> bool) {
        assert_eq!(self.pred.len(), g.module_count(), "graph size changed");
        for m in self.visited.drain(..) {
            self.pred[m.idx()] = None;
        }
        self.reached.clear();
        for &s in starts {
            if self.reached.insert(s.idx()) {
                self.visited.push(s);
            }
        }
        // `visited` doubles as the queue: it only grows, and `head` walks it
        let mut head = 0;
        while let Some(&m) = self.visited.get(head) {
            head += 1;
            for id in g.out_edge_ids(m) {
                let e = g.edge(id);
                if keep(e) && self.reached.insert(e.to.idx()) {
                    self.pred[e.to.idx()] = Some(id);
                    self.visited.push(e.to);
                }
            }
        }
    }
}

/// The modules with a path of at least one edge to some module of `targets`.
pub fn can_reach(g: &Graph, targets: &BitSet) -> BitSet {
    let mut found = BitSet::new(g.module_count());
    let mut queue: Vec<ModuleId> = vec![];
    let visit_dependents = |m: ModuleId, found: &mut BitSet, queue: &mut Vec<ModuleId>| {
        for &id in g.in_edge_ids(m) {
            let from = g.edge(id).from;
            if found.insert(from.idx()) {
                queue.push(from);
            }
        }
    };
    for t in targets.iter() {
        visit_dependents(ModuleId(t as u32), &mut found, &mut queue);
    }
    while let Some(m) = queue.pop() {
        visit_dependents(m, &mut found, &mut queue);
    }
    found
}
