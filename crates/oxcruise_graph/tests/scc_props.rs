#![allow(clippy::needless_range_loop)] // brute-force oracles read clearer with indices

use oxcruise_core::{DepKinds, Edge, EdgeId, Graph, Module, ModuleId, ModuleKind, ModuleSystem};
use oxcruise_graph::analyses::{reach_from, shortest_cycle, strongly_connected};
use proptest::prelude::*;

fn build(n: usize, pairs: &[(usize, usize)]) -> Graph {
    let modules = (0..n)
        .map(|i| Module::new(format!("m{i:02}.ts"), ModuleKind::Source))
        .collect();
    let edges = pairs
        .iter()
        .enumerate()
        .map(|(i, &(a, b))| Edge {
            from: ModuleId(a as u32),
            to: ModuleId(b as u32),
            specifier: format!("./{i}"),
            kinds: DepKinds::LOCAL | DepKinds::IMPORT,
            module_system: ModuleSystem::Es6,
            loc: None,
            suppressed: vec![],
        })
        .collect();
    Graph::build(modules, edges)
}

/// Floyd-Warshall transitive closure; `reach[a][b]` = path of length >= 1.
fn closure(g: &Graph) -> Vec<Vec<bool>> {
    let n = g.module_count();
    let mut r = vec![vec![false; n]; n];
    for e in g.edges() {
        r[e.from.idx()][e.to.idx()] = true;
    }
    for k in 0..n {
        for i in 0..n {
            for j in 0..n {
                if r[i][k] && r[k][j] {
                    r[i][j] = true;
                }
            }
        }
    }
    r
}

/// Length of the shortest cycle through edge `e`, by brute-force BFS on the whole graph.
fn brute_shortest_cycle_len(g: &Graph, e: EdgeId) -> Option<usize> {
    let ed = g.edge(e);
    if ed.from == ed.to {
        return Some(1);
    }
    let n = g.module_count();
    let mut dist = vec![usize::MAX; n];
    dist[ed.to.idx()] = 0;
    let mut frontier = vec![ed.to];
    while !frontier.is_empty() {
        let mut next = vec![];
        for m in frontier {
            for x in g.out_edges(m) {
                if dist[x.to.idx()] == usize::MAX {
                    dist[x.to.idx()] = dist[m.idx()] + 1;
                    next.push(x.to);
                }
            }
        }
        frontier = next;
    }
    (dist[ed.from.idx()] != usize::MAX).then(|| dist[ed.from.idx()] + 1)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn scc_matches_brute_force(n in 1usize..9, raw in prop::collection::vec((0usize..9, 0usize..9), 0..30)) {
        let pairs: Vec<_> = raw.into_iter().filter(|&(a, b)| a < n && b < n).collect();
        let g = build(n, &pairs);
        let scc = strongly_connected(&g, &|_| true);
        let r = closure(&g);
        for a in 0..n {
            for b in 0..n {
                let same = a == b || (r[a][b] && r[b][a]);
                prop_assert_eq!(scc.component(ModuleId(a as u32)) == scc.component(ModuleId(b as u32)), same);
            }
        }
        for (i, e) in g.edges().iter().enumerate() {
            let id = EdgeId(i as u32);
            let on_cycle = r[e.to.idx()][e.from.idx()] || e.from == e.to;
            prop_assert_eq!(scc.is_cyclic_edge(&g, id), on_cycle);

            let cycle = shortest_cycle(&g, &scc, id, &|_| true);
            prop_assert_eq!(cycle.is_some(), on_cycle);
            if let Some(steps) = cycle {
                // starts at `from`, follows real edges, and closes
                prop_assert_eq!(steps[0].module, e.from);
                prop_assert_eq!(steps[0].edge, Some(id));
                for (k, s) in steps.iter().enumerate() {
                    let edge = g.edge(s.edge.unwrap());
                    prop_assert_eq!(edge.from, s.module);
                    let next = steps[(k + 1) % steps.len()].module;
                    prop_assert_eq!(edge.to, next);
                }
                prop_assert_eq!(Some(steps.len()), brute_shortest_cycle_len(&g, id));
            }
        }
    }

    #[test]
    fn reach_matches_closure(n in 1usize..9, raw in prop::collection::vec((0usize..9, 0usize..9), 0..30), start in 0usize..9) {
        let pairs: Vec<_> = raw.into_iter().filter(|&(a, b)| a < n && b < n).collect();
        let g = build(n, &pairs);
        let start = start % n;
        let reach = reach_from(&g, &[ModuleId(start as u32)], &|_| true);
        let r = closure(&g);
        for b in 0..n {
            prop_assert_eq!(reach.contains(ModuleId(b as u32)), b == start || r[start][b]);
            if b != start && reach.contains(ModuleId(b as u32)) {
                let path = reach.path_to(&g, ModuleId(b as u32)).unwrap();
                prop_assert_eq!(path[0].module, ModuleId(start as u32));
                prop_assert_eq!(path.last().unwrap().module, ModuleId(b as u32));
            }
        }
    }

    #[test]
    fn type_only_filter_removes_cycles(n in 2usize..7) {
        // ring where one edge is type-only: no cycle when type-only edges are ignored
        let pairs: Vec<_> = (0..n).map(|i| (i, (i + 1) % n)).collect();
        let mut g_edges: Vec<Edge> = build(n, &pairs).edges().to_vec();
        g_edges[0].kinds |= DepKinds::TYPE_ONLY;
        let modules = (0..n).map(|i| Module::new(format!("m{i:02}.ts"), ModuleKind::Source)).collect();
        let g = Graph::build(modules, g_edges);
        let all = strongly_connected(&g, &|_| true);
        let runtime = strongly_connected(&g, &|e| !e.is_type_only());
        prop_assert!(g.edges().iter().enumerate().all(|(i, _)| all.is_cyclic_edge(&g, EdgeId(i as u32))));
        prop_assert!(g.edges().iter().enumerate().all(|(i, _)| !runtime.is_cyclic_edge(&g, EdgeId(i as u32))));
    }
}
