// rill-fft/benches/convolver_bench.rs
//! Benchmarks for convolution methods comparison.
//!
//! Run with: `cargo bench -p rill-fft --bench convolver_bench`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rill_fft::overlap_add::OverlapAddConvolver;
use rill_fft::partitioned_conv::PartitionedConvolver;

const BUF_SIZE: usize = 128;

fn bench_ola(c: &mut Criterion) {
    let ir_lens = [256, 512, 1024, 2048, 4096, 8192];

    let mut group = c.benchmark_group("conv_overlap_add_f32");
    for ir_len in ir_lens {
        let ir: Vec<f32> = (0..ir_len).map(|i| 0.99f32.powi(i as i32)).collect();
        let mut conv = OverlapAddConvolver::<f32, BUF_SIZE>::new(ir_len);
        conv.set_ir(&ir);

        let input = vec![0.5f32; BUF_SIZE];
        let mut output = vec![0.0f32; BUF_SIZE];

        group.throughput(Throughput::Elements(BUF_SIZE as u64));
        group.bench_with_input(BenchmarkId::from_parameter(ir_len), &ir_len, |bench, _| {
            bench.iter(|| {
                conv.process(black_box(&input), black_box(&mut output));
            });
        });
    }
    group.finish();
}

fn bench_partitioned(c: &mut Criterion) {
    let ir_lens = [4096, 16384, 65536];

    let mut group = c.benchmark_group("conv_partitioned_f32");
    for ir_len in ir_lens {
        let ir: Vec<f32> = (0..ir_len).map(|i| 0.999f32.powi(i as i32)).collect();
        let mut conv = PartitionedConvolver::<f32, BUF_SIZE>::new(ir_len);
        conv.set_ir(&ir);

        let input = vec![0.5f32; BUF_SIZE];
        let mut output = vec![0.0f32; BUF_SIZE];

        group.throughput(Throughput::Elements(BUF_SIZE as u64));
        group.bench_with_input(BenchmarkId::from_parameter(ir_len), &ir_len, |bench, _| {
            bench.iter(|| {
                conv.process(black_box(&input), black_box(&mut output));
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_ola, bench_partitioned);
criterion_main!(benches);
