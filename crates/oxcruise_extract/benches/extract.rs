use std::hint::black_box;
use std::path::Path;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use oxc_allocator::Allocator;
use oxcruise_extract::{ExtractOptions, extract};

/// A TypeScript module with `n` imports of various shapes plus the usual body.
fn module(n: usize) -> String {
    let mut s = String::new();
    for i in 0..n {
        match i % 5 {
            0 => s.push_str(&format!("import {{ a{i}, b{i} }} from \"./dep{i}\";\n")),
            1 => s.push_str(&format!("import type {{ T{i} }} from \"./types{i}\";\n")),
            2 => s.push_str(&format!("export {{ x{i} }} from \"./re{i}\";\n")),
            3 => s.push_str(&format!("const l{i} = () => import(\"./lazy{i}\");\n")),
            _ => s.push_str(&format!("const r{i} = require(\"./cjs{i}\");\n")),
        }
    }
    for i in 0..n * 4 {
        s.push_str(&format!(
            "export function f{i}(a: number, b: string): Promise<{{ ok: boolean }}> {{\n  // comment {i}\n  return fetch(`/api/${{a}}/${{b}}`).then((r) => ({{ ok: r.ok }}));\n}}\n"
        ));
    }
    s
}

fn bench(c: &mut Criterion) {
    let opts = ExtractOptions::default();
    let mut group = c.benchmark_group("extract");
    for n in [10, 100, 400] {
        let src = module(n);
        group.throughput(Throughput::Bytes(src.len() as u64));
        let mut alloc = Allocator::default();
        group.bench_function(format!("ts-{n}-imports"), |b| {
            b.iter(|| {
                alloc.reset();
                black_box(extract(Path::new("a.ts"), black_box(&src), &opts, &alloc))
            })
        });
    }
    group.finish();
}

criterion_group!(benches, bench);
criterion_main!(benches);
