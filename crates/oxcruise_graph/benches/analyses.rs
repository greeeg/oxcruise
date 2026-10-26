use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use oxcruise_core::{DepKinds, Edge, EdgeId, Graph, Module, ModuleId, ModuleKind, ModuleSystem};
use oxcruise_graph::analyses::{reach_from, shortest_cycle, strongly_connected};
use rayon::prelude::*;

/// `n` modules, ~6 edges each: mostly to nearby modules, some far, so there is one big
/// component plus many small ones, like a real codebase.
fn parts(n: usize) -> (Vec<Module>, Vec<Edge>) {
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let modules = (0..n)
        .map(|i| Module::new(format!("src/m{i:07}.ts"), ModuleKind::Source))
        .collect();
    let mut edges = Vec::with_capacity(n * 6);
    for i in 0..n {
        for k in 0..6 {
            let to = if next() % 5 == 0 {
                (next() % n as u64) as usize
            } else {
                (i + (next() % 80) as usize).saturating_sub(40).min(n - 1)
            };
            if to != i {
                edges.push(Edge {
                    from: ModuleId(i as u32),
                    to: ModuleId(to as u32),
                    specifier: format!("./{k}"),
                    kinds: DepKinds::LOCAL | DepKinds::IMPORT,
                    module_system: ModuleSystem::Es6,
                    loc: None,
                    suppressed: vec![],
                });
            }
        }
    }
    (modules, edges)
}

fn bench(c: &mut Criterion) {
    let n = 50_000;
    let (modules, edges) = parts(n);
    let mut group = c.benchmark_group("graph-50k");
    group.sample_size(20);

    group.bench_function("freeze (sort + CSR)", |b| {
        b.iter_batched(
            || (modules.clone(), edges.clone()),
            |(m, e)| black_box(Graph::build(m, e)),
            BatchSize::LargeInput,
        )
    });

    let g = Graph::build(modules, edges);
    group.bench_function("tarjan scc", |b| {
        b.iter(|| black_box(strongly_connected(&g, &|_| true)))
    });

    let scc = strongly_connected(&g, &|_| true);
    group.bench_function("shortest cycle for every cyclic edge (parallel)", |b| {
        b.iter(|| {
            let found = (0..g.edge_count() as u32)
                .into_par_iter()
                .filter(|&e| scc.is_cyclic_edge(&g, EdgeId(e)))
                .filter_map(|e| shortest_cycle(&g, &scc, EdgeId(e), &|_| true))
                .count();
            black_box(found)
        })
    });

    group.bench_function("reach from one module", |b| {
        b.iter(|| black_box(reach_from(&g, &[ModuleId(0)], &|_| true)))
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
