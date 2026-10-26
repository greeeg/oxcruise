use oxcruise_core::{Graph, ModuleId};

/// Robert Martin's coupling metrics for one module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Afferent coupling: distinct modules that depend on this one.
    pub ca: u32,
    /// Efferent coupling: distinct modules this one depends on.
    pub ce: u32,
    /// `ce / (ca + ce)`; zero for isolated modules.
    pub instability: f64,
}

pub fn module_metrics(g: &Graph, m: ModuleId) -> Metrics {
    let ca = distinct_dependents(g, m);
    let ce = distinct_dependencies(g, m);
    let instability = if ca + ce == 0 {
        0.0
    } else {
        f64::from(ce) / f64::from(ca + ce)
    };
    Metrics {
        ca,
        ce,
        instability,
    }
}

/// Distinct modules depending on `m` (self-edges excluded). In-edges are
/// ordered by source module, so distinct sources are adjacent.
pub fn distinct_dependents(g: &Graph, m: ModuleId) -> u32 {
    let mut last = None;
    let mut n = 0;
    for &id in g.in_edge_ids(m) {
        let from = g.edge(id).from;
        if from != m && last != Some(from) {
            n += 1;
            last = Some(from);
        }
    }
    n
}

pub fn distinct_dependencies(g: &Graph, m: ModuleId) -> u32 {
    let mut last = None;
    let mut n = 0;
    for e in g.out_edges(m) {
        if e.to != m && last != Some(e.to) {
            n += 1;
            last = Some(e.to);
        }
    }
    n
}
