//! Alignment impact bench — FixedBuffer (16-byte aligned) vs raw Vec.
//!
//! Run with: `cargo bench -p rill-core --bench vector_bench`

use criterion::{criterion_group, criterion_main, Criterion};
use rill_core::buffer::FixedBuffer;
use rill_core::math::vector::scalar::ScalarVector4;
use rill_core::math::vector::traits::Vector as VecTrait;

const N: usize = 256;

fn bench_aligned_fixedbuffer(c: &mut Criterion) {
    let mut buf: FixedBuffer<f32, N> = FixedBuffer::new();
    for (i, v) in buf.as_mut_array().iter_mut().enumerate() {
        *v = (i as f32) * 0.01;
    }
    let mut out = vec![0.0f32; N];

    c.bench_function("aligned_FixedBuffer", |bench| {
        bench.iter(|| {
            let arr = buf.as_array();
            let chunks = N / 4;
            for c in 0..chunks {
                let o = c * 4;
                let v = ScalarVector4::load(&arr[o..o + 4]);
                let s = v.mul(&ScalarVector4::splat(2.0));
                s.store(&mut out[o..o + 4]);
            }
        });
    });
}

fn bench_unaligned_vec_offset(c: &mut Criterion) {
    // Force misaligned access by offsetting by 1 f32 (4 bytes)
    let padded: Vec<f32> = vec![0.0; N + 4];
    let data = &padded[1..]; // offset by 4 bytes = unaligned for SIMD
    let mut out = vec![0.0f32; N];

    c.bench_function("unaligned_vec_offset1", |bench| {
        bench.iter(|| {
            let chunks = N / 4;
            for c in 0..chunks {
                let o = c * 4;
                let v = ScalarVector4::load(&data[o..o + 4]);
                let s = v.mul(&ScalarVector4::splat(2.0));
                s.store(&mut out[o..o + 4]);
            }
        });
    });
}

fn bench_aligned_vec(c: &mut Criterion) {
    let data: Vec<f32> = (0..N).map(|i| (i as f32) * 0.01).collect();
    let mut out = vec![0.0f32; N];

    c.bench_function("aligned_vec", |bench| {
        bench.iter(|| {
            let chunks = N / 4;
            for c in 0..chunks {
                let o = c * 4;
                let v = ScalarVector4::load(&data[o..o + 4]);
                let s = v.mul(&ScalarVector4::splat(2.0));
                s.store(&mut out[o..o + 4]);
            }
        });
    });
}

criterion_group!(
    benches,
    bench_aligned_fixedbuffer,
    bench_aligned_vec,
    bench_unaligned_vec_offset
);
criterion_main!(benches);
