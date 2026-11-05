use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use oxcruise_core::{
    DepKinds, Edge, FileTree, Graph, Module, ModuleId, ModuleKind, ModuleSystem, UsedNames,
};
use oxcruise_rules::testing::Tester;
use serde_json::json;

const FEATURES: usize = 1_000;
const FILES: usize = 50;

fn path(f: usize, m: usize) -> String {
    format!("src/features/f{f:04}/m{m:03}.ts")
}

/// `FEATURES` folders of `FILES` modules. Each module imports its neighbour in the folder, and the
/// first module of a folder imports the next folder's (and every tenth, the previous one's, so the
/// folders form cycles).
fn graph() -> Graph {
    let modules: Vec<Module> = (0..FEATURES)
        .flat_map(|f| (0..FILES).map(move |m| Module::new(path(f, m), ModuleKind::Local)))
        .collect();
    let id = |f: usize, m: usize| ModuleId((f * FILES + m) as u32);
    let mut edges = vec![];
    let mut edge = |from: ModuleId, to: ModuleId| {
        edges.push(Edge {
            from,
            to,
            specifier: "./x".into(),
            kinds: DepKinds::LOCAL | DepKinds::IMPORT,
            module_system: ModuleSystem::Es6,
            loc: None,
            suppressed: vec![],
            used: UsedNames::All,
        })
    };
    for f in 0..FEATURES {
        for m in 1..FILES {
            edge(id(f, m), id(f, m - 1));
        }
        edge(id(f, 0), id((f + 1) % FEATURES, 0));
        if f % 10 == 0 {
            edge(id((f + 1) % FEATURES, 1), id(f, 1));
        }
    }
    Graph::build(modules, edges)
}

fn bench(c: &mut Criterion) {
    let g = graph();
    let files: Vec<String> = (0..FEATURES)
        .flat_map(|f| (0..FILES).map(move |m| path(f, m)))
        .collect();
    let mut group = c.benchmark_group("folder-rules-50k");
    group.sample_size(10);
    group.bench_function("file tree", |b| {
        b.iter(|| black_box(FileTree::new(files.clone())))
    });
    for (name, rule, options) in [
        (
            "folder-structure: require and allow with a capture",
            "folder-structure",
            json!({ "folders": [
                { "path": "src/features/{f}", "require": "m000.ts", "allow": ["m*.ts", "{f}.md"], "case": "snake-case" }
            ] }),
        ),
        (
            "folder-structure: a glob that matches every file",
            "folder-structure",
            json!({ "folders": [{ "path": "src/**", "allow": "!tmp" }] }),
        ),
        (
            "visibility",
            "visibility",
            json!({ "folders": [{ "path": "src/features/{f}", "public": "m000.ts" }] }),
        ),
        (
            "circular-dependencies: folders",
            "circular-dependencies",
            json!({ "folders": [{ "path": "src/features/{f}" }] }),
        ),
    ] {
        let t = Tester::rule(rule, options);
        group.bench_function(name, |b| b.iter(|| black_box(t.run_graph(&g))));
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
