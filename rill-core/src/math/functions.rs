//! Common mathematical functions for signal processing

use super::num::Transcendental;
use super::vector::scalar::ScalarVector4;
use super::vector::traits::Vector as VecTrait;

/// Linear interpolation
#[inline(always)]
pub fn lerp<T: Transcendental>(a: T, b: T, t: T) -> T {
    a + (b - a) * t
}

/// Convert decibels to a linear coefficient
#[inline(always)]
pub fn db_to_linear<T: Transcendental>(db: T) -> T {
    T::from_f32(10.0_f32.powf(db.to_f32() / 20.0))
}

/// Convert a linear coefficient to decibels
#[inline(always)]
pub fn linear_to_db<T: Transcendental>(linear: T) -> T {
    T::from_f32(20.0 * linear.to_f32().log10())
}
/// Convert seconds to samples
#[inline(always)]
pub fn seconds_to_samples(seconds: f32, sample_rate: f32) -> usize {
    (seconds * sample_rate) as usize
}

/// Convert samples to seconds
#[inline(always)]
pub fn samples_to_seconds(samples: usize, sample_rate: f32) -> f32 {
    samples as f32 / sample_rate
}

/// Fast tanh approximation
#[inline(always)]
pub fn fast_tanh<T: Transcendental>(x: T) -> T {
    let xf = x.to_f32();
    T::from_f32(xf / (1.0 + xf.abs()))
}

/// Soft clipping
#[inline(always)]
pub fn soft_clip<T: Transcendental>(x: T, threshold: T) -> T {
    let xf = x.to_f32();
    let t = threshold.to_f32();

    if xf > t {
        T::from_f32(t + (xf - t) / (1.0 + ((xf - t) / (1.0 - t)).powi(2)))
    } else if xf < -t {
        T::from_f32(-t - (-xf - t) / (1.0 + ((-xf - t) / (1.0 - t)).powi(2)))
    } else {
        x
    }
}

/// Hann window
#[inline(always)]
pub fn hann_window<T: Transcendental>(x: T) -> T {
    let cos_term = (x * T::from_f32(2.0) * T::PI).cos();
    T::from_f32(0.5) * (T::ONE - cos_term)
}

/// Convert f32 chunk to i16 with SIMD clamping and scaling.
///
/// Each f32 sample is clamped to `[-1.0, 1.0]`, multiplied by 32767,
/// and truncated to `i16`. Processes 4 samples at a time.
///
/// # Panics
/// Panics if `dst` is shorter than `src`.
pub fn f32_to_i16_chunk(src: &[f32], dst: &mut [i16]) {
    let len = src.len().min(dst.len());
    let chunks = len / 4;

    for chunk in 0..chunks {
        let o = chunk * 4;
        let v = ScalarVector4::load(&src[o..o + 4]);
        let lo = ScalarVector4::splat(-1.0f32);
        let hi = ScalarVector4::splat(1.0f32);
        let scale = ScalarVector4::splat(32767.0f32);
        let clamped = v.clamp(&lo, &hi);
        let scaled = clamped.mul(&scale);
        dst[o] = scaled.extract(0) as i16;
        dst[o + 1] = scaled.extract(1) as i16;
        dst[o + 2] = scaled.extract(2) as i16;
        dst[o + 3] = scaled.extract(3) as i16;
    }

    for i in chunks * 4..len {
        dst[i] = (src[i].clamp(-1.0, 1.0) * 32767.0) as i16;
    }
}

/// Convert i16 chunk to f32 with SIMD scaling.
///
/// Each i16 sample is divided by 32768.0 to produce f32 in `[-1.0, 1.0)`.
/// Processes 4 samples at a time.
///
/// # Panics
/// Panics if `dst` is shorter than `src`.
pub fn i16_to_f32_chunk(src: &[i16], dst: &mut [f32]) {
    let len = src.len().min(dst.len());
    let chunks = len / 4;

    for chunk in 0..chunks {
        let o = chunk * 4;
        let v = ScalarVector4::load(&[
            src[o] as f32 / 32768.0,
            src[o + 1] as f32 / 32768.0,
            src[o + 2] as f32 / 32768.0,
            src[o + 3] as f32 / 32768.0,
        ]);
        v.store(&mut dst[o..o + 4]);
    }

    for i in chunks * 4..len {
        dst[i] = src[i] as f32 / 32768.0;
    }
}

/// Deinterleave stereo interleaved buffer into two mono buffers (SIMD).
///
/// `stereo` contains `[L0,R0, L1,R1, L2,R2, ...]`.
/// Processes 4 stereo pairs (8 samples) per chunk.
///
/// # Panics
/// Panics if `out_l` or `out_r` is shorter than `stereo.len() / 2`.
pub fn deinterleave_stereo(stereo: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
    let pairs = (stereo.len() / 2).min(out_l.len()).min(out_r.len());
    let chunks = pairs / 4;

    for chunk in 0..chunks {
        let so = chunk * 8;
        let mo = chunk * 4;

        let v01 = ScalarVector4::load(&stereo[so..so + 4]);
        let v23 = ScalarVector4::load(&stereo[so + 4..so + 8]);

        let l = ScalarVector4::from_fn(|i| {
            if i < 2 {
                v01.extract(i * 2)
            } else {
                v23.extract((i - 2) * 2)
            }
        });

        let r = ScalarVector4::from_fn(|i| {
            if i < 2 {
                v01.extract(i * 2 + 1)
            } else {
                v23.extract((i - 2) * 2 + 1)
            }
        });

        l.store(&mut out_l[mo..mo + 4]);
        r.store(&mut out_r[mo..mo + 4]);
    }

    for i in chunks * 4..pairs {
        out_l[i] = stereo[i * 2];
        out_r[i] = stereo[i * 2 + 1];
    }
}

/// Interleave two mono buffers into a stereo interleaved buffer (SIMD).
///
/// Produces `[L0,R0, L1,R1, L2,R2, ...]`.
/// Processes 4 stereo pairs (8 samples) per chunk.
///
/// # Panics
/// Panics if `stereo` is shorter than `2 * in_l.len()`.
pub fn interleave_stereo(in_l: &[f32], in_r: &[f32], stereo: &mut [f32]) {
    let pairs = in_l.len().min(in_r.len()).min(stereo.len() / 2);
    let chunks = pairs / 4;

    for chunk in 0..chunks {
        let mo = chunk * 4;
        let so = chunk * 8;

        let l = ScalarVector4::load(&in_l[mo..mo + 4]);
        let r = ScalarVector4::load(&in_r[mo..mo + 4]);

        let out: [f32; 8] = std::array::from_fn(|i| {
            if i % 2 == 0 {
                l.extract(i / 2)
            } else {
                r.extract(i / 2)
            }
        });

        stereo[so..so + 8].copy_from_slice(&out);
    }

    for i in chunks * 4..pairs {
        stereo[i * 2] = in_l[i];
        stereo[i * 2 + 1] = in_r[i];
    }
}
