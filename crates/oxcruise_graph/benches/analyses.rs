use std::hint::black_box;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use oxcruise_core::{
    DepKinds, Edge, EdgeId, Export, ExportKind, Graph, Loc, Module, ModuleId, ModuleKind,
    ModuleSystem, UsedNames,
};
use oxcruise_graph::analyses::{
    BitSet, export_usage, reach_from, shortest_cycle, strongly_connected,
};
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
        .map(|i| Module::new(format!("src/m{i:07}.ts"), ModuleKind::Local))
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
                    used: oxcruise_core::UsedNames::All,
                });
            }
        }
    }
    (modules, edges)
}

/// `parts` with symbols: every module exports `e0..e7`, re-exports `e0` of its first import as
/// `r0`, and each import reads a few names, `r0` among them, so marks travel through re-exports.
fn symbol_parts(n: usize) -> (Vec<Module>, Vec<Edge>) {
    let (mut modules, mut edges) = parts(n);
    let export = |name: String, kind, from: Option<u32>, imported: Option<&str>| Export {
        name,
        kind,
        is_type: false,
        from: from.map(ModuleId),
        imported: imported.map(Into::into),
        loc: Loc::default(),
        suppressed: vec![],
        members: vec![],
        used_in_file: false,
        duplicate_of: None,
        fix: None,
    };
    let mut first_import: Vec<Option<u32>> = vec![None; n];
    for e in &mut edges {
        first_import[e.from.idx()].get_or_insert(e.to.0);
        let k = e.to.idx() % 4;
        e.used = UsedNames::Names(
            [
                format!("e{k}"),
                format!("e{}", k + 1),
                "r0".to_string(),
                format!("E{k}.A"),
            ]
            .into_iter()
            .map(String::into_boxed_str)
            .collect(),
        );
    }
    for (i, m) in modules.iter_mut().enumerate() {
        m.exports = (0..8)
            .map(|k| export(format!("e{k}"), ExportKind::Local, None, None))
            .collect();
        m.exports
            .push(export("E0".into(), ExportKind::Local, None, None));
        if let Some(to) = first_import[i] {
            m.exports.push(export(
                "r0".into(),
                ExportKind::Reexport,
                Some(to),
                Some("e0"),
            ));
        }
        m.exports.sort_by(|a, b| a.name.cmp(&b.name));
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

    let (modules, edges) = symbol_parts(n);
    let g = Graph::build(modules, edges);
    let mut entries = BitSet::new(g.module_count());
    for m in (0..n).step_by(500) {
        entries.insert(m);
    }
    let mut group = c.benchmark_group("symbols-50k");
    group.sample_size(20);
    group.bench_function("export usage", |b| {
        b.iter(|| black_box(export_usage(&g, &entries, &|_| false)))
    });
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
