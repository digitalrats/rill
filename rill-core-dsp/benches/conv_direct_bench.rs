//! Benchmarks for direct convolution.
//!
//! Run with: `cargo bench -p rill-core-dsp --bench conv_direct_bench`

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rill_core::traits::algorithm::Algorithm;
use rill_core_dsp::DirectConvolver;

const BUF_SIZE: usize = 128;

fn bench_direct_ir_len(c: &mut Criterion) {
    let ir_lens = [8, 16, 32, 64, 128];

    let mut group = c.benchmark_group("direct_conv_f32");
    for ir_len in ir_lens {
        let ir: Vec<f32> = (0..ir_len).map(|i| 0.9f32.powi(i as i32)).collect();
        let mut conv = DirectConvolver::<f32, 128, BUF_SIZE>::new();
        conv.set_ir(&ir);

        let input = vec![0.5f32; BUF_SIZE];
        let mut output = vec![0.0f32; BUF_SIZE];

        group.throughput(Throughput::Elements(BUF_SIZE as u64));
        group.bench_with_input(BenchmarkId::from_parameter(ir_len), &ir_len, |bench, _| {
            bench.iter(|| {
                conv.process(Some(black_box(&input)), black_box(&mut output))
                    .unwrap();
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_direct_ir_len);
criterion_main!(benches);
