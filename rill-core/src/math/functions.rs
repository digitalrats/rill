//! Common mathematical functions for audio processing

use super::num::Transcendental;

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

/// Convert a MIDI note to frequency
#[inline(always)]
pub fn midi_to_freq<T: Transcendental>(note: u8) -> T {
    let exp = (note as f32 - 69.0) / 12.0;
    T::from_f32(440.0 * 2.0_f32.powf(exp))
}

/// Convert frequency to a MIDI note
#[inline(always)]
pub fn freq_to_midi<T: Transcendental>(freq: T) -> f32 {
    69.0 + 12.0 * (freq.to_f32() / 440.0).log2()
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
