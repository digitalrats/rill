//! Benchmarks for rill-core-dsp filters.
//!
//! Run with: `cargo bench -p rill-core-dsp --bench filter_bench`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rill_core::time::ClockTick;
use rill_core::traits::ActionContext;
use rill_core_dsp::filters::{Biquad, Filter, FilterParams, FilterType};
use rill_core_dsp::Algorithm;

const BLOCK: usize = 256;
const SR: f32 = 44100.0;

fn ctx() -> ActionContext<'static> {
    let tick = Box::leak(Box::new(ClockTick::new(0, BLOCK as u32, SR)));
    ActionContext::new(tick)
}

fn make_input() -> Vec<f32> {
    (0..BLOCK).map(|i| (i as f32 * 0.01).sin()).collect()
}

fn bench_biquad_lp(c: &mut Criterion) {
    let params = FilterParams {
        filter_type: FilterType::LowPass,
        cutoff: 1000.0,
        q: 0.707,
        gain_db: 0.0,
    };
    let mut filter = Biquad::<f32>::new(params);
    filter.init(SR);
    let input = make_input();
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("biquad_lowpass", |bench| {
        bench.iter(|| {
            filter
                .process(Some(black_box(&input)), black_box(&mut out), &ctx)
                .unwrap();
        });
    });
}

fn bench_biquad_hp(c: &mut Criterion) {
    let params = FilterParams {
        filter_type: FilterType::HighPass,
        cutoff: 500.0,
        q: 0.707,
        gain_db: 0.0,
    };
    let mut filter = Biquad::<f32>::new(params);
    filter.init(SR);
    let input = make_input();
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("biquad_highpass", |bench| {
        bench.iter(|| {
            filter
                .process(Some(black_box(&input)), black_box(&mut out), &ctx)
                .unwrap();
        });
    });
}

fn bench_biquad_peak(c: &mut Criterion) {
    let params = FilterParams {
        filter_type: FilterType::Peak,
        cutoff: 1000.0,
        q: 2.0,
        gain_db: 6.0,
    };
    let mut filter = Biquad::<f32>::new(params);
    filter.init(SR);
    let input = make_input();
    let ctx = ctx();
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("biquad_peak", |bench| {
        bench.iter(|| {
            filter
                .process(Some(black_box(&input)), black_box(&mut out), &ctx)
                .unwrap();
        });
    });
}

criterion_group!(benches, bench_biquad_lp, bench_biquad_hp, bench_biquad_peak);
criterion_main!(benches);
