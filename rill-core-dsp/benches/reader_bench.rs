//! Benchmarks for rill-core-dsp interpolated reader and resampler.
//!
//! Run with: `cargo bench -p rill-core-dsp --bench reader_bench`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rill_core::time::ClockTick;
use rill_core::traits::ActionContext;
use rill_core_dsp::generators::{InterpolatedReader, Resampler};
use rill_core_dsp::Algorithm;

const BLOCK: usize = 256;

fn bench_reader_linear(c: &mut Criterion) {
    let buf: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.0001).sin()).collect();
    let mut reader = InterpolatedReader::new(buf);
    reader.set_rate(44100.0 / 48000.0); // 44.1k → 48k
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("reader_linear", |bench| {
        bench.iter(|| {
            reader.render_block(black_box(&mut out));
        });
    });
}

fn bench_reader_cubic(c: &mut Criterion) {
    let buf: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.0001).sin()).collect();
    let mut reader = InterpolatedReader::new(buf);
    reader.set_cubic(true);
    reader.set_rate(44100.0 / 48000.0);
    let mut out = vec![0.0f32; BLOCK];

    c.bench_function("reader_cubic", |bench| {
        bench.iter(|| {
            reader.render_block(black_box(&mut out));
        });
    });
}

fn bench_resampler_44k_to_48k(c: &mut Criterion) {
    let buf: Vec<f32> = (0..44100).map(|i| (i as f32 * 0.0001).sin()).collect();
    let mut rs = Resampler::new(buf, 44100.0);
    rs.set_cubic(true);
    rs.init(48000.0);
    let mut out = vec![0.0f32; BLOCK];
    let ctx = {
        use rill_core::time::ClockTick;
        use rill_core::traits::ActionContext;
        let tick = Box::leak(Box::new(ClockTick::new(0, BLOCK as u32, 48000.0)));
        ActionContext::new(tick)
    };

    c.bench_function("resampler_44k1_to_48k", |bench| {
        bench.iter(|| {
            rs.process(None, black_box(&mut out), &ctx).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_reader_linear,
    bench_reader_cubic,
    bench_resampler_44k_to_48k
);
criterion_main!(benches);
