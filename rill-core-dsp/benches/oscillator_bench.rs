//! Benchmarks for rill-core-dsp oscillators.
//!
//! Run with: `cargo bench -p rill-core-dsp --bench oscillator_bench`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rill_core::time::ClockTick;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ActionContext;
use rill_core_dsp::generators::{BasicOscillator, Waveform};

const BLOCK: usize = 256;
const SR: f32 = 44100.0;

fn ctx() -> ActionContext<'static> {
    let tick = Box::leak(Box::new(ClockTick::new(0, BLOCK as u32, SR)));
    ActionContext::new(tick)
}

fn bench_sine(c: &mut Criterion) {
    let mut osc = BasicOscillator::<f32>::new(Waveform::Sine, 440.0, 0.5);
    osc.init(SR);
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("osc_sine", |bench| {
        bench.iter(|| {
            osc.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

fn bench_saw(c: &mut Criterion) {
    let mut osc = BasicOscillator::<f32>::new(Waveform::Saw, 440.0, 0.5);
    osc.init(SR);
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("osc_saw", |bench| {
        bench.iter(|| {
            osc.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

fn bench_square(c: &mut Criterion) {
    let mut osc = BasicOscillator::<f32>::new(Waveform::Square, 440.0, 0.5);
    osc.init(SR);
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("osc_square", |bench| {
        bench.iter(|| {
            osc.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

fn bench_triangle(c: &mut Criterion) {
    let mut osc = BasicOscillator::<f32>::new(Waveform::Triangle, 440.0, 0.5);
    osc.init(SR);
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("osc_triangle", |bench| {
        bench.iter(|| {
            osc.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

fn bench_pulse(c: &mut Criterion) {
    let mut osc = BasicOscillator::<f32>::new(Waveform::Pulse(0.25), 440.0, 0.5);
    osc.init(SR);
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("osc_pulse", |bench| {
        bench.iter(|| {
            osc.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_sine,
    bench_saw,
    bench_square,
    bench_triangle,
    bench_pulse
);
criterion_main!(benches);
