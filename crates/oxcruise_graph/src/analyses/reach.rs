use std::collections::VecDeque;

use oxcruise_core::{CycleStep, Edge, EdgeId, Graph, ModuleId};

use super::BitSet;

/// Everything reachable from a set of start modules, with the edge that
/// first reached each module (so a path can be reported).
#[derive(Debug, Clone)]
pub struct Reach {
    pub reached: BitSet,
    pred: Vec<Option<EdgeId>>,
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
    let n = g.module_count();
    let mut reached = BitSet::new(n);
    let mut pred = vec![None; n];
    let mut q = VecDeque::new();
    for &s in starts {
        if reached.insert(s.idx()) {
            q.push_back(s);
        }
    }
    while let Some(m) = q.pop_front() {
        for id in g.out_edge_ids(m) {
            let e = g.edge(id);
            if keep(e) && reached.insert(e.to.idx()) {
                pred[e.to.idx()] = Some(id);
                q.push_back(e.to);
            }
        }
    }
    Reach { reached, pred }
}
