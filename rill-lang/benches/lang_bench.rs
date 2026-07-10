//! Benchmarks for rill-lang: compilation and hybrid runtime throughput.
//!
//! Run with: `cargo bench -p rill-lang --bench lang_bench`
//!
//! The `*_vs_reference` groups quantify the block-processing win: `process`
//! (the hybrid block/sample executor) versus `process_reference` (the
//! per-sample interpreter oracle) on the same program and input.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use rill_core::traits::Algorithm;
use rill_lang::{compile, RillProgram};

const BLOCK: usize = 256;

fn make_input() -> Vec<f32> {
    (0..BLOCK).map(|i| (i as f32 * 0.017).sin() * 0.7).collect()
}

fn build(src: &str) -> RillProgram<f32> {
    compile::<f32>(src).expect("program compiles")
}

// ---------------------------------------------------------------------------
// Compilation throughput (lex → parse → HM → lower → schedule).
// ---------------------------------------------------------------------------

fn bench_compile(c: &mut Criterion) {
    let programs = [
        ("gain", "main = _ * 0.5"),
        ("chain", "main = _ * 0.5 : abs : (_ * 2.0)"),
        ("feedback", "main = + ~ (_ * 0.5)"),
        (
            "mixed",
            "main = (_ * 0.5) <: (_ , _ * 0.5) :> (+ ~ (_ * 0.7))",
        ),
    ];
    let mut group = c.benchmark_group("compile");
    for (name, src) in programs {
        group.bench_with_input(BenchmarkId::from_parameter(name), &src, |b, &src| {
            b.iter(|| {
                let prog = compile::<f32>(black_box(src)).unwrap();
                black_box(prog);
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Hybrid runtime throughput per program class.
// ---------------------------------------------------------------------------

fn bench_runtime(c: &mut Criterion) {
    let input = make_input();
    let programs = [
        ("feedforward_gain", "main = _ * 0.5"),
        ("feedforward_chain", "main = _ * 0.5 : abs : (_ * 2.0)"),
        ("feedback_leaky", "main = + ~ (_ * 0.5)"),
        ("delay", "main = _ @ 4"),
        ("split_merge", "main = _ <: (_ , _ * 0.5) :> +"),
        ("param", "main g = _ * g"),
        ("smooth", "main g = smooth g 10.0"),
    ];
    let mut group = c.benchmark_group("runtime");
    group.throughput(criterion::Throughput::Elements(BLOCK as u64));
    for (name, src) in programs {
        let mut prog = build(src);
        let mut out = vec![0.0f32; BLOCK];
        group.bench_function(name, |b| {
            b.iter(|| {
                prog.process(Some(black_box(&input)), black_box(&mut out))
                    .unwrap();
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Hybrid vs. per-sample reference — the block-processing speedup.
// ---------------------------------------------------------------------------

fn bench_hybrid_vs_reference(c: &mut Criterion) {
    let input = make_input();
    let cases = [
        (
            "feedforward",
            "main = _ * 0.5 : abs : (_ * 2.0) : (_ + 0.1)",
        ),
        ("feedback", "main = + ~ (_ * 0.9)"),
    ];
    for (name, src) in cases {
        let mut group = c.benchmark_group(format!("hybrid_vs_reference/{name}"));
        group.throughput(criterion::Throughput::Elements(BLOCK as u64));

        let mut hybrid = build(src);
        let mut out = vec![0.0f32; BLOCK];
        group.bench_function("hybrid", |b| {
            b.iter(|| {
                hybrid
                    .process(Some(black_box(&input)), black_box(&mut out))
                    .unwrap();
            });
        });

        let mut reference = build(src);
        let mut out_ref = vec![0.0f32; BLOCK];
        group.bench_function("reference", |b| {
            b.iter(|| {
                reference
                    .process_reference(Some(black_box(&input)), black_box(&mut out_ref))
                    .unwrap();
            });
        });
        group.finish();
    }
}

criterion_group!(
    benches,
    bench_compile,
    bench_runtime,
    bench_hybrid_vs_reference
);
criterion_main!(benches);
