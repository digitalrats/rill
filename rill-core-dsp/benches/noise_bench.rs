//! Benchmarks for rill-core-dsp noise generators.
//!
//! Run with: `cargo bench -p rill-core-dsp --bench noise_bench`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rill_core::time::ClockTick;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ActionContext;
use rill_core_dsp::generators::{NoiseGenerator, NoiseType};

const BLOCK: usize = 256;
const SR: f32 = 44100.0;

fn ctx() -> ActionContext<'static> {
    let tick = Box::leak(Box::new(ClockTick::new(0, BLOCK as u32, SR)));
    ActionContext::new(tick)
}

fn bench_noise_white(c: &mut Criterion) {
    let mut gen = NoiseGenerator::<f32>::new(NoiseType::White, 0.5);
    gen.init(SR);
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("noise_white", |bench| {
        bench.iter(|| {
            gen.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

fn bench_noise_brown(c: &mut Criterion) {
    let mut gen = NoiseGenerator::<f32>::new(NoiseType::Brown, 0.5);
    gen.init(SR);
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("noise_brown", |bench| {
        bench.iter(|| {
            gen.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

fn bench_noise_blue(c: &mut Criterion) {
    let mut gen = NoiseGenerator::<f32>::new(NoiseType::Blue, 0.5);
    gen.init(SR);
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("noise_blue", |bench| {
        bench.iter(|| {
            gen.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

fn bench_noise_violet(c: &mut Criterion) {
    let mut gen = NoiseGenerator::<f32>::new(NoiseType::Violet, 0.5);
    gen.init(SR);
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("noise_violet", |bench| {
        bench.iter(|| {
            gen.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_noise_white,
    bench_noise_brown,
    bench_noise_blue,
    bench_noise_violet
);
criterion_main!(benches);
