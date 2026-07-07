//! Benchmarks for rill-fft complex FFT.
//!
//! Run with: `cargo bench -p rill-fft --bench fft_complex_bench`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use num_complex::Complex;
use rill_fft::complex_fft::ComplexFft;

fn bench_fft_size(c: &mut Criterion) {
    let sizes = [64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384];

    let mut group = c.benchmark_group("fft_complex_f32");
    for size in sizes {
        let fft = ComplexFft::<f32>::new(size);
        let mut data: Vec<Complex<f32>> = (0..size)
            .map(|i| {
                let x = i as f32 * 0.1;
                Complex::new(x.sin(), x.cos())
            })
            .collect();

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |bench, _| {
            bench.iter(|| {
                fft.forward(black_box(&mut data));
            });
        });
    }
    group.finish();
}

fn bench_fft_f64(c: &mut Criterion) {
    let sizes = [64, 128, 256, 512, 1024, 2048, 4096, 8192];

    let mut group = c.benchmark_group("fft_complex_f64");
    for size in sizes {
        let fft = ComplexFft::<f64>::new(size);
        let mut data: Vec<Complex<f64>> = (0..size)
            .map(|i| {
                let x = i as f64 * 0.1;
                Complex::new(x.sin(), x.cos())
            })
            .collect();

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |bench, _| {
            bench.iter(|| {
                fft.forward(black_box(&mut data));
            });
        });
    }
    group.finish();
}

fn bench_fft_inverse(c: &mut Criterion) {
    let sizes = [64, 256, 1024, 4096, 16384];

    let mut group = c.benchmark_group("fft_complex_inverse_f32");
    for size in sizes {
        let fft = ComplexFft::<f32>::new(size);
        let mut data: Vec<Complex<f32>> = (0..size)
            .map(|i| {
                let x = i as f32 * 0.1;
                Complex::new(x.sin(), x.cos())
            })
            .collect();

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |bench, _| {
            bench.iter(|| {
                fft.inverse(black_box(&mut data));
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_fft_size, bench_fft_f64, bench_fft_inverse);
criterion_main!(benches);
