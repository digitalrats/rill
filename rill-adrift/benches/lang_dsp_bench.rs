//! Benchmarks for rill-lang DSP built-ins compiled through `rill-adrift`.
//!
//! Run with: `cargo bench -p rill-adrift --features lang --bench lang_dsp_bench`
//!
//! Measures the overhead of driving `rill-core-dsp` filters through the DSL
//! versus running the raw `Algorithm` directly, plus sample- vs block-level
//! built-ins and dynamic (`param`) parameterization.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rill_adrift::lang_builtins::full_registry;
use rill_core::traits::Algorithm;
use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};
use rill_lang::compile_with;

const BLOCK: usize = 256;
const SR: f32 = 48_000.0;

fn make_input() -> Vec<f32> {
    (0..BLOCK).map(|i| (i as f32 * 0.05).sin() * 0.5).collect()
}

fn bench_builtins(c: &mut Criterion) {
    let reg = full_registry::<f32>();
    let input = make_input();
    let programs = [
        ("lowpass_block", "process = _ : lowpass(1000.0, 0.7);"),
        ("moog_sample", "process = _ : moog(800.0, 0.6);"),
        ("onepole_sample", "process = _ : onepole(1200.0, 0.5);"),
        (
            "dynamic_cutoff",
            "process = _ : lowpass(param(\"cutoff\", 1000.0), 0.7);",
        ),
        (
            "filter_chain",
            "process = _ : lowpass(2000.0, 0.7) : moog(500.0, 0.6);",
        ),
    ];
    let mut group = c.benchmark_group("builtins");
    group.throughput(criterion::Throughput::Elements(BLOCK as u64));
    for (name, src) in programs {
        let mut prog = compile_with::<f32>(src, &reg, SR).expect("compiles");
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

/// DSL-wrapped biquad vs. the raw `Biquad` algorithm — the DSL overhead.
fn bench_dsl_vs_raw(c: &mut Criterion) {
    let reg = full_registry::<f32>();
    let input = make_input();

    let mut group = c.benchmark_group("biquad_dsl_vs_raw");
    group.throughput(criterion::Throughput::Elements(BLOCK as u64));

    let mut dsl = compile_with::<f32>("process = _ : lowpass(1000.0, 0.7);", &reg, SR).unwrap();
    let mut out = vec![0.0f32; BLOCK];
    group.bench_function("dsl", |b| {
        b.iter(|| {
            dsl.process(Some(black_box(&input)), black_box(&mut out))
                .unwrap();
        });
    });

    let mut raw = Biquad::<f32>::new(FilterParams {
        filter_type: FilterType::LowPass,
        cutoff: 1000.0,
        q: 0.7,
        gain_db: 0.0,
    });
    raw.init(SR);
    let mut out_raw = vec![0.0f32; BLOCK];
    group.bench_function("raw", |b| {
        b.iter(|| {
            raw.process(Some(black_box(&input)), black_box(&mut out_raw))
                .unwrap();
        });
    });
    group.finish();
}

criterion_group!(benches, bench_builtins, bench_dsl_vs_raw);
criterion_main!(benches);
