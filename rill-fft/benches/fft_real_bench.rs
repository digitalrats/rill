//! Benchmarks for real FFT.
//!
//! Run with: `cargo bench -p rill-fft --bench fft_real_bench`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rill_fft::real_fft::RealFft;

fn bench_real_fft(c: &mut Criterion) {
    let sizes = [64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384];

    let mut group = c.benchmark_group("fft_real_f32");
    for size in sizes {
        let mut fft = RealFft::<f32>::new(size);
        let input: Vec<f32> = (0..size).map(|i| (i as f32 * 0.1).sin()).collect();
        let mut spectrum = vec![num_complex::Complex::new(0.0, 0.0); size / 2 + 1];

        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |bench, _| {
            bench.iter(|| {
                fft.forward(black_box(&input), black_box(&mut spectrum));
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_real_fft);
criterion_main!(benches);
