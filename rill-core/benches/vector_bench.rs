//! Benchmarks for rill-core vector operations.
//!
//! Compares scalar vs. SIMD paths for base math on 1024-element slices.
//! Run with: `cargo bench -p rill-core`

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rill_core::math::vector::scalar::ScalarVector4;
use rill_core::math::vector::traits::{Vector as VecTrait, VectorTranscendental};

const N: usize = 1024;

fn bench_add_scalar(c: &mut Criterion) {
    let a = vec![1.0f32; N];
    let b = vec![2.0f32; N];
    let mut out = vec![0.0f32; N];

    c.bench_function("vector_add_scalar", |bench| {
        bench.iter(|| {
            for i in 0..N {
                out[i] = black_box(a[i] + b[i]);
            }
        });
    });

    c.bench_function("vector_add_simd4", |bench| {
        bench.iter(|| {
            let chunks = N / 4;
            for c in 0..chunks {
                let o = c * 4;
                let av = ScalarVector4::load(&a[o..o + 4]);
                let bv = ScalarVector4::load(&b[o..o + 4]);
                av.add(&bv).store(&mut out[o..o + 4]);
            }
            for i in chunks * 4..N {
                out[i] = a[i] + b[i];
            }
        });
    });
}

fn bench_mul_scalar(c: &mut Criterion) {
    let a = vec![3.0f32; N];
    let b = vec![0.5f32; N];
    let mut out = vec![0.0f32; N];

    c.bench_function("vector_mul_scalar", |bench| {
        bench.iter(|| {
            for i in 0..N {
                out[i] = black_box(a[i] * b[i]);
            }
        });
    });

    c.bench_function("vector_mul_simd4", |bench| {
        bench.iter(|| {
            let chunks = N / 4;
            for c in 0..chunks {
                let o = c * 4;
                let av = ScalarVector4::load(&a[o..o + 4]);
                let bv = ScalarVector4::load(&b[o..o + 4]);
                av.mul(&bv).store(&mut out[o..o + 4]);
            }
        });
    });
}

fn bench_sin_scalar(c: &mut Criterion) {
    let a: Vec<f32> = (0..N).map(|i| (i as f32) * 0.01).collect();
    let mut out = vec![0.0f32; N];

    c.bench_function("vector_sin_scalar", |bench| {
        bench.iter(|| {
            for i in 0..N {
                out[i] = black_box(a[i].sin());
            }
        });
    });

    c.bench_function("vector_sin_simd4", |bench| {
        bench.iter(|| {
            let chunks = N / 4;
            for c in 0..chunks {
                let o = c * 4;
                let v = ScalarVector4::load(&a[o..o + 4]);
                v.sin().store(&mut out[o..o + 4]);
            }
            for i in chunks * 4..N {
                out[i] = a[i].sin();
            }
        });
    });
}

fn bench_clamp_scalar(c: &mut Criterion) {
    let a: Vec<f32> = (0..N).map(|i| (i as f32 - 512.0) / 256.0).collect();
    let mut out = vec![0.0f32; N];

    c.bench_function("vector_clamp_scalar", |bench| {
        bench.iter(|| {
            for i in 0..N {
                out[i] = black_box(a[i].clamp(-1.0, 1.0));
            }
        });
    });

    c.bench_function("vector_clamp_simd4", |bench| {
        bench.iter(|| {
            let lo = ScalarVector4::splat(-1.0f32);
            let hi = ScalarVector4::splat(1.0f32);
            let chunks = N / 4;
            for c in 0..chunks {
                let o = c * 4;
                let v = ScalarVector4::load(&a[o..o + 4]);
                v.clamp(&lo, &hi).store(&mut out[o..o + 4]);
            }
        });
    });
}

criterion_group!(
    benches,
    bench_add_scalar,
    bench_mul_scalar,
    bench_sin_scalar,
    bench_clamp_scalar,
);
criterion_main!(benches);
