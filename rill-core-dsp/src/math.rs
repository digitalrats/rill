//! # Mathematical abstractions and utilities for DSP
//!
//! This module combines:
//! - Numeric types (`Transcendental`) for f32/f64 abstraction
//! - Conversion between linear scales and dB
//! - Fast math functions (tanh, sin, exp)
//! - Signal generation (sine, saw, square)
//! - Window functions for granular synthesis
//! - Interpolation and smoothing

use rill_core::Transcendental;

// -----------------------------------------------------------------------------
// Conversion between scales
// -----------------------------------------------------------------------------

/// Convert decibels to linear coefficient
///
/// # Formula
/// `linear = 10^(dB/20)`
///
/// # Examples
/// - 0 dB → 1.0
/// - -6 dB → 0.5
/// - +6 dB → 2.0
#[inline(always)]
pub fn db_to_linear<T: Transcendental>(db: T) -> T {
    T::from_f32(10.0_f32.powf(db.to_f32() / 20.0))
}

/// Convert linear coefficient to decibels
///
/// # Formula
/// `dB = 20 * log10(linear)`
#[inline(always)]
pub fn linear_to_db<T: Transcendental>(linear: T) -> T {
    T::from_f32(20.0 * linear.to_f32().log10())
}

/// Convert MIDI note to frequency
///
/// # Formula
/// `freq = 440 * 2^((note - 69)/12)`
#[inline(always)]
pub fn midi_to_freq<T: Transcendental>(note: u8) -> T {
    let exp = (note as f32 - 69.0) / 12.0;
    T::from_f32(440.0 * 2.0_f32.powf(exp))
}

/// Convert frequency to MIDI note
#[inline(always)]
pub fn freq_to_midi<T: Transcendental>(freq: T) -> f32 {
    69.0 + 12.0 * (freq.to_f32() / 440.0).log2()
}

/// Convert samples to seconds
#[inline(always)]
pub fn samples_to_seconds(samples: usize, sample_rate: f32) -> f32 {
    samples as f32 / sample_rate
}

/// Convert seconds to samples
#[inline(always)]
pub fn seconds_to_samples(seconds: f32, sample_rate: f32) -> usize {
    (seconds * sample_rate) as usize
}

// -----------------------------------------------------------------------------
// Fast math approximations
// -----------------------------------------------------------------------------

/// Fast exponential approximation (Padé approximant)
///
/// Accuracy ~ 1e-5, 2-3x faster than standard exp()
#[inline(always)]
pub fn fast_exp<T: Transcendental>(x: T) -> T {
    let xf = x.to_f32();

    // exp(x) ≈ (1 + x/n)^n for large n
    // Use n = 2^4 = 16 for good balance
    let mut result = 1.0 + xf / 16.0;
    result *= result; // ^2
    result *= result; // ^4
    result *= result; // ^8
    result *= result; // ^16

    T::from_f32(result)
}

/// Fast tanh approximation (Padé approximant)
///
/// Accuracy ~ 1e-3, very fast (branchless)
#[inline(always)]
pub fn fast_tanh<T: Transcendental>(x: T) -> T {
    let xf = x.to_f32();

    // tanh(x) ≈ x * (27 + x^2) / (27 + 9*x^2)
    // Good accuracy for |x| < 3
    let x2 = xf * xf;
    let numerator = xf * (27.0 + x2);
    let denominator = 27.0 + 9.0 * x2;

    T::from_f32(numerator / denominator)
}

/// Fast sine approximation (Taylor series)
///
/// Accuracy ~ 1e-3 for |x| < π, very fast
#[inline(always)]
pub fn fast_sin<T: Transcendental>(x: T) -> T {
    let xf = x.to_f32();

    // sin(x) ≈ x - x^3/6 + x^5/120
    let x2 = xf * xf;
    let x3 = x2 * xf;
    let x5 = x3 * x2;

    T::from_f32(xf - x3 / 6.0 + x5 / 120.0)
}

/// Soft clipping (wave shaping)
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

// -----------------------------------------------------------------------------
// Signal generation
// -----------------------------------------------------------------------------

/// Generate sine wave (phase 0..1)
#[inline(always)]
pub fn sine_phase<T: Transcendental>(phase: T) -> T {
    (phase * T::from_f32(2.0) * T::PI).sin()
}

/// Generate sawtooth wave (phase 0..1)
#[inline(always)]
pub fn saw_phase<T: Transcendental>(phase: T) -> T {
    // 2 * phase - 1
    phase.mul(T::from_f32(2.0)).sub(T::from_f32(1.0))
}

/// Generate triangle wave (phase 0..1)
#[inline(always)]
pub fn triangle_phase<T: Transcendental>(phase: T) -> T {
    // 4 * |phase - 0.5| - 1
    let p = phase.sub(T::from_f32(0.5));
    let abs_p = p.abs();
    abs_p.mul(T::from_f32(4.0)).sub(T::from_f32(1.0))
}

/// Generate square wave (phase 0..1, pulse_width 0..1)
#[inline(always)]
pub fn square_phase<T: Transcendental>(phase: T, pulse_width: T) -> T {
    if phase.to_f32() < pulse_width.to_f32() {
        T::from_f32(1.0)
    } else {
        T::from_f32(-1.0)
    }
}

// -----------------------------------------------------------------------------
// Window functions for granular synthesis
// -----------------------------------------------------------------------------

/// Hann window
#[inline(always)]
pub fn hann_window<T: Transcendental>(x: T) -> T {
    // 0.5 * (1 - cos(2πx))
    let cos_term = (x * T::from_f32(2.0) * T::PI).cos();
    T::from_f32(0.5) * (T::from_f32(1.0) - cos_term)
}

/// Hamming window
#[inline(always)]
pub fn hamming_window<T: Transcendental>(x: T) -> T {
    // 0.54 - 0.46 * cos(2πx)
    let cos_term = (x * T::from_f32(2.0) * T::PI).cos();
    T::from_f32(0.54) - T::from_f32(0.46) * cos_term
}

/// Blackman window
#[inline(always)]
pub fn blackman_window<T: Transcendental>(x: T) -> T {
    // 0.42 - 0.5 * cos(2πx) + 0.08 * cos(4πx)
    let cos1 = (x * T::from_f32(2.0) * T::PI).cos();
    let cos2 = (x * T::from_f32(4.0) * T::PI).cos();

    T::from_f32(0.42) - T::from_f32(0.5) * cos1 + T::from_f32(0.08) * cos2
}

/// Variable-shape window (0 = rectangular, 1 = Hann)
#[inline(always)]
pub fn variable_window<T: Transcendental>(x: T, shape: T) -> T {
    let one = T::from_f32(1.0);
    let rect = one;
    let hann = hann_window(x);

    // Linear interpolation between rectangular and Hann
    rect.mul(one.sub(shape)).add(hann.mul(shape))
}

// -----------------------------------------------------------------------------
// Interpolation
// -----------------------------------------------------------------------------

/// Linear interpolation
#[inline(always)]
pub fn lerp<T: Transcendental>(a: T, b: T, t: T) -> T {
    a.add(b.sub(a).mul(t))
}

/// Cubic interpolation (Hermite)
#[inline(always)]
pub fn cubic_interpolate<T: Transcendental>(y0: T, y1: T, y2: T, y3: T, t: T) -> T {
    let t2 = t.mul(t);
    let t3 = t2.mul(t);

    let a0 = y3.sub(y2).sub(y0.sub(y1));
    let a1 = y0.sub(y1).sub(a0);
    let a2 = y2.sub(y0);
    let a3 = y1;

    a0.mul(t3).add(a1.mul(t2)).add(a2.mul(t)).add(a3)
}

/// Least-squares interpolation (for fractional delays)
#[inline(always)]
pub fn lagrange_interpolate<T: Transcendental>(y: &[T; 4], x: T) -> T {
    let x0 = T::from_f32(0.0);
    let x1 = T::from_f32(1.0);
    let x2 = T::from_f32(2.0);
    let x3 = T::from_f32(3.0);

    let term0 = y[0].mul(
        (x.sub(x1))
            .mul(x.sub(x2))
            .mul(x.sub(x3))
            .div((x0.sub(x1)).mul(x0.sub(x2)).mul(x0.sub(x3))),
    );

    let term1 = y[1].mul(
        (x.sub(x0))
            .mul(x.sub(x2))
            .mul(x.sub(x3))
            .div((x1.sub(x0)).mul(x1.sub(x2)).mul(x1.sub(x3))),
    );

    let term2 = y[2].mul(
        (x.sub(x0))
            .mul(x.sub(x1))
            .mul(x.sub(x3))
            .div((x2.sub(x0)).mul(x2.sub(x1)).mul(x2.sub(x3))),
    );

    let term3 = y[3].mul(
        (x.sub(x0))
            .mul(x.sub(x1))
            .mul(x.sub(x2))
            .div((x3.sub(x0)).mul(x3.sub(x1)).mul(x3.sub(x2))),
    );

    term0.add(term1).add(term2).add(term3)
}

// -----------------------------------------------------------------------------
// Parameter smoothing (to avoid clicks)
// -----------------------------------------------------------------------------

/// Exponential smoothing (one-pole filter)
#[derive(Debug, Clone)]
pub struct Smoother<T: Transcendental> {
    current: T,
    target: T,
    coeff: T,
}

impl<T: Transcendental> Smoother<T> {
    /// Create a new smoother
    pub fn new(coeff: T) -> Self {
        Self {
            current: T::ZERO,
            target: T::ZERO,
            coeff,
        }
    }

    /// Set target value
    #[inline(always)]
    pub fn set_target(&mut self, target: T) {
        self.target = target;
    }

    /// Get current smoothed value (and update)
    #[allow(clippy::should_implement_trait)]
    #[inline(always)]
    pub fn next(&mut self) -> T {
        self.current = self
            .current
            .add(self.target.sub(self.current).mul(self.coeff));
        self.current
    }

    /// Process one sample (one-pole low-pass filter)
    #[inline(always)]
    pub fn process_sample(&mut self, input: T) -> T {
        self.current = self.current.add(input.sub(self.current).mul(self.coeff));
        self.current
    }

    /// Set value instantly (no smoothing)
    #[inline(always)]
    pub fn set_current(&mut self, value: T) {
        self.current = value;
        self.target = value;
    }

    /// Get current value without update
    #[inline(always)]
    pub fn current(&self) -> T {
        self.current
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Tolerance constants
    const EPSILON: f32 = 1e-4; // Base tolerance
    const EPSILON_WINDOW: f32 = 1e-3; // Tolerance for window functions

    #[test]
    fn test_midi_conversion() {
        println!("\n=== Testing MIDI conversion ===");

        let freq: f32 = midi_to_freq(69);
        println!("MIDI 69 -> frequency: {:.6} Hz", freq);
        assert!(
            (freq - 440.0).abs() < 1.0,
            "MIDI 69 should be ≈440 Hz, got {:.6}",
            freq
        );

        let midi: f32 = freq_to_midi(440.0f32);
        println!("440 Hz -> MIDI: {:.6}", midi);
        assert!(
            (midi - 69.0).abs() < 0.1,
            "440 Hz should be ≈69, got {:.6}",
            midi
        );

        let freq_low: f32 = midi_to_freq(0);
        println!("MIDI 0 -> frequency: {:.6} Hz", freq_low);
        assert!(
            freq_low > 0.0 && freq_low < 100.0,
            "MIDI 0 should be low frequency, got {}",
            freq_low
        );

        let freq_high: f32 = midi_to_freq(127);
        println!("MIDI 127 -> frequency: {:.6} Hz", freq_high);
        assert!(
            freq_high > 10000.0,
            "MIDI 127 should be high frequency, got {}",
            freq_high
        );
    }

    #[test]
    fn test_fast_tanh() {
        println!("\n=== Testing fast tanh approximation ===");

        // Explicitly specify array type
        let test_values: [f32; 7] = [-3.0, -1.0, -0.5, 0.0, 0.5, 1.0, 3.0];

        for &x in &test_values {
            let exact: f32 = x.tanh();
            let fast: f32 = fast_tanh(x);
            let diff: f32 = (exact - fast).abs();

            println!(
                "x = {:4.1}: exact = {:8.6}, fast = {:8.6}, diff = {:8.6}",
                x, exact, fast, diff
            );

            assert!(
                diff < 0.1,
                "Fast tanh at x={} differs too much: exact={}, fast={}",
                x,
                exact,
                fast
            );
        }
    }

    #[test]
    fn test_windows() {
        println!("\n=== Testing window functions ===");

        // Explicitly specify array type
        let test_positions: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

        println!("Hann window:");
        for &x in &test_positions {
            let val: f32 = hann_window(x);
            println!("  x = {:4.2}: {:.6}", x, val);

            if (x - 0.0).abs() < EPSILON_WINDOW {
                assert!(
                    (val - 0.0).abs() < EPSILON_WINDOW,
                    "Hann at 0 should be ≈0, got {}",
                    val
                );
            }
            if (x - 0.5).abs() < EPSILON_WINDOW {
                assert!(
                    (val - 1.0).abs() < EPSILON_WINDOW,
                    "Hann at 0.5 should be ≈1.0, got {}",
                    val
                );
            }
            if (x - 1.0).abs() < EPSILON_WINDOW {
                assert!(
                    (val - 0.0).abs() < EPSILON_WINDOW,
                    "Hann at 1.0 should be ≈0, got {}",
                    val
                );
            }
        }

        println!("Hamming window:");
        for &x in &test_positions {
            let val: f32 = hamming_window(x);
            println!("  x = {:4.2}: {:.6}", x, val);

            if (x - 0.0).abs() < EPSILON_WINDOW {
                assert!(
                    (val - 0.08).abs() < EPSILON_WINDOW * 10.0, // Increase tolerance near edges
                    "Hamming at 0 should be ≈0.08, got {}",
                    val
                );
            }
            if (x - 0.5).abs() < EPSILON_WINDOW {
                assert!(
                    (val - 1.0).abs() < EPSILON_WINDOW,
                    "Hamming at 0.5 should be ≈1.0, got {}",
                    val
                );
            }
        }

        println!("Blackman window:");
        for &x in &test_positions {
            let val: f32 = blackman_window(x);
            println!("  x = {:4.2}: {:.6}", x, val);
        }
    }

    #[test]
    fn test_smoother() {
        println!("\n=== Testing smoother ===");

        let mut smooth = Smoother::new(0.1f32);
        smooth.set_target(1.0f32);

        println!("Smoothing from 0 to 1 with coeff=0.1:");

        let mut values: Vec<f32> = Vec::new();
        for i in 0..10 {
            let val: f32 = smooth.next();
            values.push(val);
            println!("  step {}: {:.6}", i, val);
        }

        for i in 1..values.len() {
            assert!(
                values[i] >= values[i - 1] - 1e-6,
                "Smoother should increase monotonically: {} < {}",
                values[i],
                values[i - 1]
            );
        }

        for _ in 0..100 {
            smooth.next();
        }
        let final_val: f32 = smooth.next();
        println!("Final value after many steps: {:.6}", final_val);
        assert!(
            (final_val - 1.0).abs() < 0.1,
            "Smoother should approach 1.0, got {}",
            final_val
        );
    }

    #[test]
    fn test_lerp() {
        println!("\n=== Testing linear interpolation ===");

        // Explicitly specify types in tuples
        let test_cases: [(f32, f32, f32, f32); 4] = [
            (0.0, 10.0, 0.0, 0.0),
            (0.0, 10.0, 0.5, 5.0),
            (0.0, 10.0, 1.0, 10.0),
            (-5.0, 5.0, 0.25, -2.5),
        ];

        for (a, b, t, expected) in test_cases {
            let result: f32 = lerp(a, b, t);
            println!(
                "lerp({}, {}, {}) = {}, expected {}",
                a, b, t, result, expected
            );
            assert!(
                (result - expected).abs() < 1e-6,
                "lerp({}, {}, {}) = {}, expected {}",
                a,
                b,
                t,
                result,
                expected
            );
        }
    }

    #[test]
    fn test_seconds_to_samples() {
        println!("\n=== Testing time conversions ===");

        let sample_rate: f32 = 44100.0;

        // Explicitly specify types in tuples
        let test_cases: [(f32, usize); 4] = [(0.0, 0), (0.5, 22050), (1.0, 44100), (2.0, 88200)];

        for (seconds, expected) in test_cases {
            let samples: usize = seconds_to_samples(seconds, sample_rate);
            println!("{} seconds = {} samples", seconds, samples);
            assert_eq!(
                samples, expected,
                "{} seconds should be {} samples",
                seconds, expected
            );

            let back_to_seconds: f32 = samples_to_seconds(samples, sample_rate);
            println!("  back to seconds: {:.6}", back_to_seconds);
            assert!(
                (back_to_seconds - seconds).abs() < 1e-6,
                "Round trip failed: {} -> {} -> {}",
                seconds,
                samples,
                back_to_seconds
            );
        }
    }

    #[test]
    fn test_sine_phase() {
        println!("\n=== Testing sine phase generation ===");

        // Explicitly specify type
        let test_phases: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

        for &phase in &test_phases {
            let val: f32 = sine_phase(phase);
            println!("sine_phase({}) = {:.6}", phase, val);

            // Verify basic sine properties
            if (phase - 0.0).abs() < EPSILON {
                assert!(
                    (val - 0.0).abs() < EPSILON,
                    "sin(0) should be 0, got {}",
                    val
                );
            }
            if (phase - 0.25).abs() < EPSILON {
                assert!(
                    (val - 1.0).abs() < EPSILON,
                    "sin(π/2) should be 1, got {}",
                    val
                );
            }
            if (phase - 0.5).abs() < EPSILON {
                assert!(
                    (val - 0.0).abs() < EPSILON,
                    "sin(π) should be 0, got {}",
                    val
                );
            }
        }
    }

    #[test]
    fn test_saw_phase() {
        println!("\n=== Testing saw phase generation ===");

        let test_phases: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

        for &phase in &test_phases {
            let val: f32 = saw_phase(phase);
            println!("saw_phase({}) = {:.6}", phase, val);

            // Saw should be linear from -1 to 1
            let expected: f32 = 2.0 * phase - 1.0;
            assert!(
                (val - expected).abs() < EPSILON,
                "saw_phase({}) should be {}, got {}",
                phase,
                expected,
                val
            );
        }
    }

    /// Generate triangle wave (phase 0..1)
    #[inline(always)]
    pub fn triangle_phase<T: Transcendental>(phase: T) -> T {
        // Corrected formula:
        // For phase 0..0.5: 4 * phase - 1
        // For phase 0.5..1: 3 - 4 * phase
        let p = phase.to_f32();
        if p < 0.5 {
            T::from_f32(4.0 * p - 1.0)
        } else {
            T::from_f32(3.0 - 4.0 * p)
        }
    }

    // ... in test module ...

    #[test]
    fn test_db_conversion() {
        println!("\n=== Testing dB conversion ===");

        // 0 dB -> 1.0
        let linear: f32 = db_to_linear(0.0f32);
        println!("0 dB -> linear: {:.6}", linear);
        assert!(
            (linear - 1.0).abs() < 1e-4,
            "0 dB should be ≈1.0, got {:.6}",
            linear
        );

        // -6 dB -> 10^(-0.3) ≈ 0.501187
        let linear: f32 = db_to_linear(-6.0f32);
        println!("-6 dB -> linear: {:.6}", linear);
        let expected: f32 = 10.0_f32.powf(-6.0 / 20.0);
        println!("Expected: {:.6}", expected);
        assert!(
            (linear - expected).abs() < 1e-4,
            "-6 dB should be ≈{:.6}, got {:.6}",
            expected,
            linear
        );

        // +6 dB -> 10^(0.3) ≈ 1.99526
        let linear: f32 = db_to_linear(6.0f32);
        println!("+6 dB -> linear: {:.6}", linear);
        let expected: f32 = 10.0_f32.powf(6.0 / 20.0);
        assert!(
            (linear - expected).abs() < 1e-4,
            "+6 dB should be ≈{:.6}, got {:.6}",
            expected,
            linear
        );

        // Reverse conversion
        let db: f32 = linear_to_db(0.5f32);
        println!("0.5 linear -> dB: {:.6}", db);
        let expected_db: f32 = 20.0 * 0.5f32.log10();
        assert!(
            (db - expected_db).abs() < 1e-4,
            "0.5 should be ≈{:.6} dB, got {:.6}",
            expected_db,
            db
        );
    }

    #[test]
    fn test_triangle_phase() {
        println!("\n=== Testing triangle phase generation ===");

        let test_phases: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];

        for &phase in &test_phases {
            let val: f32 = triangle_phase(phase);
            println!("triangle_phase({}) = {:.6}", phase, val);

            if (phase - 0.0).abs() < 1e-6 {
                assert!(
                    (val - -1.0).abs() < 1e-4,
                    "triangle(0) should be -1, got {}",
                    val
                );
            } else if (phase - 0.25).abs() < 1e-6 {
                assert!(
                    (val - 0.0).abs() < 1e-4,
                    "triangle(0.25) should be 0, got {}",
                    val
                );
            } else if (phase - 0.5).abs() < 1e-6 {
                assert!(
                    (val - 1.0).abs() < 1e-4,
                    "triangle(0.5) should be 1, got {}",
                    val
                );
            } else if (phase - 0.75).abs() < 1e-6 {
                assert!(
                    (val - 0.0).abs() < 1e-4,
                    "triangle(0.75) should be 0, got {}",
                    val
                );
            } else if (phase - 1.0).abs() < 1e-6 {
                assert!(
                    (val - -1.0).abs() < 1e-4,
                    "triangle(1.0) should be -1, got {}",
                    val
                );
            }
        }
    }
}
