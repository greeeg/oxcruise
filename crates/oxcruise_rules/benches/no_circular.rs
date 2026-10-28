use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use oxcruise_core::{DepKinds, Edge, Graph, Module, ModuleId, ModuleKind, ModuleSystem};
use oxcruise_rules::testing::Tester;
use serde_json::json;

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
    let (modules, edges) = parts(50_000);
    let g = Graph::build(modules, edges);
    let mut group = c.benchmark_group("no-circular-50k");
    group.sample_size(10);
    // nearly every module sits in one giant component: the worst case for the rule
    for (name, options) in [
        ("default limit (100 per component)", json!(null)),
        ("no limit", json!({ "maxCyclesPerComponent": 0 })),
    ] {
        let t = Tester::rule("no-circular", options);
        group.bench_function(name, |b| b.iter(|| black_box(t.run_graph(&g))));
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
