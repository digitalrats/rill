//! RT timing benchmarks for worst-case latency analysis.
//!
//! Run with: `cargo bench -p rill-fft --bench rt_timing_bench`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use num_complex::Complex;
use rill_fft::complex_fft::ComplexFft;
use rill_fft::overlap_add::OverlapAddConvolver;
use rill_fft::partitioned_conv::PartitionedConvolver;
use rill_fft::real_fft::RealFft;

fn bench_rt_fft_complex(c: &mut Criterion) {
    let fft = ComplexFft::<f32>::new(1024);
    let mut data: Vec<Complex<f32>> = (0..1024)
        .map(|i| {
            let x = i as f32 * 0.1;
            Complex::new(x.sin(), x.cos())
        })
        .collect();

    c.bench_function("rt_fft_complex_1024", |bench| {
        bench.iter(|| {
            fft.forward(black_box(&mut data));
        });
    });
}

fn bench_rt_fft_real(c: &mut Criterion) {
    let mut fft = RealFft::<f32>::new(1024);
    let input: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut spectrum = vec![Complex::new(0.0, 0.0); 513];

    c.bench_function("rt_fft_real_1024", |bench| {
        bench.iter(|| {
            fft.forward(black_box(&input), black_box(&mut spectrum));
        });
    });
}

fn bench_rt_ola_2048(c: &mut Criterion) {
    let mut conv = OverlapAddConvolver::<f32, 128>::new(2048);
    let ir: Vec<f32> = (0..2048).map(|i| 0.999f32.powi(i as i32)).collect();
    conv.set_ir(&ir);
    let input = vec![0.5f32; 128];
    let mut output = vec![0.0f32; 128];

    c.bench_function("rt_ola_ir2048", |bench| {
        bench.iter(|| {
            conv.process(black_box(&input), black_box(&mut output));
        });
    });
}

fn bench_rt_part_65536(c: &mut Criterion) {
    let mut conv = PartitionedConvolver::<f32, 128>::new(65536);
    let ir: Vec<f32> = (0..65536).map(|i| 0.9999f32.powi(i as i32)).collect();
    conv.set_ir(&ir);
    let input = vec![0.5f32; 128];
    let mut output = vec![0.0f32; 128];

    c.bench_function("rt_part_ir65536", |bench| {
        bench.iter(|| {
            conv.process(black_box(&input), black_box(&mut output));
        });
    });
}

criterion_group!(
    benches,
    bench_rt_fft_complex,
    bench_rt_fft_real,
    bench_rt_ola_2048,
    bench_rt_part_65536,
);
criterion_main!(benches);
