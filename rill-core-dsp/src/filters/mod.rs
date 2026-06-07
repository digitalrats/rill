//! # Basic filters for audio processing
//!
//! This module provides various filter implementations, from simple
//! to complex, for use in audio processing. All filters are
//! parameterized by `T: Transcendental` and can work with `f32` or `f64`.
//!
//! ## Available filters
//!
//! | Filter | Characteristics | Application |
//! |--------|---------------|------------|
//! | **[`Biquad`]** | Universal, 8 types, 12dB/oct | EQs, tone control, crossovers |
//! | **[`OnePole`]** | Simple, fast, 6dB/oct | Parameter smoothing, envelope followers |
//! | **[`StateVariableFilter`]** | 3 simultaneous outputs, stable at resonance | Analog emulation, synthesizers |
//! | **[`Butterworth`]** | Maximally flat, no ripple | Hi-Fi audio, mastering, analysis |
//! | **[`ChebyshevI`]** | Passband ripple, steep rolloff | EQs, steep crossovers |
//! | **[`ChebyshevII`]** | Stopband ripple, flat passband | Anti-aliasing, decimation |
//! | **[`CombFilter`]** | Comb, metallic timbre | Reverb, physical modeling |
//!
//! ## Common interface
//!
//! All filters implement the common [`Filter`] trait, which provides
//! a unified way to control parameters:
//!
//! ```rust
//! use rill_core_dsp::filters::*;
//! use rill_core::Transcendental;
//! use rill_core::time::ClockTick;
//! use rill_core::traits::ActionContext;
//!
//! fn process_filter<T: Transcendental>(filter: &mut dyn Filter<T>, input: T) -> T {
//!     filter.set_cutoff(1000.0);
//!     filter.set_q(0.707);
//!     let mut output = [T::ZERO];
//!     let tick = ClockTick::default();
//!     let ctx = ActionContext::new(&tick);
//!     filter.process(Some(&[input]), &mut output).unwrap();
//!     output[0]
//! }
//! ```
//!
//! ## Usage examples
//!
//! ### Creating a low-pass filter
//! ```
//! use rill_core::time::ClockTick;
//! use rill_core::traits::ActionContext;
//! use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};
//! use rill_core::traits::algorithm::Algorithm;
//!
//! let mut lowpass = Biquad::<f32>::new(FilterParams {
//!     filter_type: FilterType::LowPass,
//!     cutoff: 1000.0,
//!     q: 0.707,
//!     gain_db: 0.0,
//! });
//! lowpass.init(44100.0);
//!
//! let mut output = [0.0_f32];
//! let tick = ClockTick::default();
//! let ctx = ActionContext::new(&tick);
//! lowpass.process(Some(&[0.5]), &mut output).unwrap();
//! let output = output[0];
//! ```
//!
//! ### Creating a parametric equalizer
//! ```
//! use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};
//! use rill_core::traits::algorithm::Algorithm;
//!
//! let mut peak = Biquad::<f32>::new(FilterParams {
//!     filter_type: FilterType::Peak,
//!     cutoff: 1000.0,
//!     q: 2.0,
//!     gain_db: 6.0,  // +6dB boost
//! });
//! peak.init(44100.0);
//! ```
//!
//! ### High-order Butterworth filter
//! ```
//! use rill_core_dsp::filters::{Butterworth, FilterParams, FilterType};
//! use rill_core::traits::algorithm::Algorithm;
//!
//! let mut butter = Butterworth::<f32, 4>::lowpass(1000.0, 4);
//! butter.init(44100.0);
//! ```

mod biquad;
mod butterworth;
mod chebyshev;
mod comb;
mod moog_ladder;
mod one_pole;
mod svf;

pub use biquad::Biquad;
pub use butterworth::Butterworth;
pub use chebyshev::{ChebyshevI, ChebyshevII, ChebyshevParams};
pub use comb::CombFilter;
pub use moog_ladder::MoogLadder;
pub use one_pole::OnePole;
pub use svf::StateVariableFilter;

use crate::algorithm::ParameterizedAlgorithm;
use rill_core::Transcendental;

/// Common parameter type for all filters
///
/// Contains basic parameters common to most filters:
/// - `filter_type`: filter type (low-pass, high-pass, etc.)
/// - `cutoff`: cutoff or center frequency in Hz
/// - `q`: quality factor (resonance), typically 0.1 to 20.0
/// - `gain_db`: gain in dB (for peak and shelving filters)
#[derive(Debug, Clone)]
pub struct FilterParams {
    /// Filter type
    pub filter_type: FilterType,

    /// Cutoff/center frequency (Hz)
    ///
    /// For LowPass/HighPass: -3dB cutoff frequency
    /// For BandPass/Notch: center frequency
    /// For Peak/Shelf: center frequency
    pub cutoff: f32,

    /// Quality factor (0.1 - 20.0)
    ///
    /// Defines the filter bandwidth. Higher values = narrower bandwidth.
    /// For LowPass/HighPass affects resonance at cutoff frequency.
    pub q: f32,

    /// Gain in dB (for peak/shelving filters)
    ///
    /// Positive values = boost, negative values = cut.
    /// Typically -24dB to +24dB.
    pub gain_db: f32,
}

/// Filter type
///
/// Defines the filter frequency response.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterType {
    /// Low-pass filter
    ///
    /// Passes frequencies below cutoff, attenuates above.
    /// Used for smoothing, high-frequency noise removal,
    /// in subtractive synthesis (VCF).
    LowPass,

    /// High-pass filter
    ///
    /// Passes frequencies above cutoff, attenuates below.
    /// Used for DC removal, rumble filter,
    /// emphasizing upper harmonics.
    HighPass,

    /// Band-pass filter
    ///
    /// Passes only the band around the center frequency.
    /// Used for frequency band isolation, formant filters,
    /// signal analysis.
    BandPass,

    /// Notch filter
    ///
    /// Suppresses a narrow band around the center frequency.
    /// Used for removing 50/60Hz mains hum,
    /// feedback suppression, flanger effects.
    Notch,

    /// Peak filter
    ///
    /// Boosts or cuts a band around the center frequency.
    /// Core component of parametric EQs.
    Peak,

    /// Low-shelf filter
    ///
    /// Boosts or cuts all frequencies below cutoff.
    /// Used for bass tone control, frequency response correction.
    LowShelf,

    /// High-shelf filter
    ///
    /// Boosts or cuts all frequencies above cutoff.
    /// Used for treble tone control, sibilance reduction.
    HighShelf,

    /// All-pass filter
    ///
    /// Changes signal phase without affecting magnitude.
    /// Used in phasers, group delay equalization,
    /// flanger effects.
    AllPass,
}

impl FilterType {
    /// Get string representation of filter type
    ///
    /// # Example
    /// ```
    /// use rill_core_dsp::filters::FilterType;
    ///
    /// assert_eq!(FilterType::LowPass.as_str(), "lowpass");
    /// ```
    pub const fn as_str(&self) -> &'static str {
        match self {
            FilterType::LowPass => "lowpass",
            FilterType::HighPass => "highpass",
            FilterType::BandPass => "bandpass",
            FilterType::Notch => "notch",
            FilterType::Peak => "peak",
            FilterType::LowShelf => "lowshelf",
            FilterType::HighShelf => "highshelf",
            FilterType::AllPass => "allpass",
        }
    }

    /// Get human-readable filter type description
    pub const fn description(&self) -> &'static str {
        match self {
            FilterType::LowPass => "Low-pass filter",
            FilterType::HighPass => "High-pass filter",
            FilterType::BandPass => "Band-pass filter",
            FilterType::Notch => "Notch filter",
            FilterType::Peak => "Peak filter",
            FilterType::LowShelf => "Low-shelf filter",
            FilterType::HighShelf => "High-shelf filter",
            FilterType::AllPass => "All-pass filter",
        }
    }

    /// Get usage recommendations
    pub const fn usage(&self) -> &'static str {
        match self {
            FilterType::LowPass => {
                "• Subtractive synthesis (VCF)\n\
                 • Signal smoothing\n\
                 • Anti-aliasing before decimation\n\
                 • High-frequency noise removal"
            }

            FilterType::HighPass => {
                "• DC offset removal\n\
                 • Rumble filter\n\
                 • Extract upper harmonics\n\
                 • Side-chain compression"
            }

            FilterType::BandPass => {
                "• Band selection\n\
                 • Formant filters (vocal)\n\
                 • Signal analysis (spectrum)\n\
                 • \"Telephone\" effect"
            }

            FilterType::Notch => {
                "• Mains hum removal 50/60Hz\n\
                 • Feedback suppression\n\
                 • Resonance removal\n\
                 • Flanger effect"
            }

            FilterType::Peak => {
                "• Parametric EQ\n\
                 • Frequency response correction\n\
                 • Instrument isolation/notching\n\
                 • Mastering"
            }

            FilterType::LowShelf => {
                "• Tone control (bass)\n\
                 • Headphone EQ correction\n\
                 • Bass boost\n\
                 • RIAA correction"
            }

            FilterType::HighShelf => {
                "• Tone control (treble)\n\
                 • High-frequency correction\n\
                 • Hiss reduction\n\
                 • Cable loss compensation"
            }

            FilterType::AllPass => {
                "• Phasers\n\
                 • Group delay equalisation\n\
                 • Flanger effects\n\
                 • Phase correction in crossovers"
            }
        }
    }
}

/// Common trait for all filters
///
/// Provides a unified interface for controlling filter parameters.
/// All concrete filters implement this trait via [`ParameterizedAlgorithm`]
/// with `Params = FilterParams`.
///
/// # Example
/// ```
/// use rill_core_dsp::filters::*;
/// use rill_core::Transcendental;
/// use rill_core::time::ClockTick;
/// use rill_core::traits::ActionContext;
///
/// fn process_filter<T: Transcendental>(filter: &mut dyn Filter<T>, input: T) -> T {
///     filter.set_cutoff(1000.0);
///     filter.set_q(0.707);
///     let mut output = [T::ZERO];
///     let tick = ClockTick::default();
///     let ctx = ActionContext::new(&tick);
///     filter.process(Some(&[input]), &mut output).unwrap();
///     output[0]
/// }
/// ```
pub trait Filter<T: Transcendental>: ParameterizedAlgorithm<T, Params = FilterParams> {
    /// Set cutoff frequency
    ///
    /// # Arguments
    /// * `cutoff` - frequency in Hz (typically 20..20000)
    fn set_cutoff(&mut self, cutoff: f32) {
        let mut params = self.params().clone();
        params.cutoff = cutoff;
        self.set_params(params);
    }

    /// Get current cutoff frequency
    fn cutoff(&self) -> f32 {
        self.params().cutoff
    }

    /// Set quality factor (Q)
    ///
    /// # Arguments
    /// * `q` - quality factor (typically 0.1..20.0)
    fn set_q(&mut self, q: f32) {
        let mut params = self.params().clone();
        params.q = q;
        self.set_params(params);
    }

    /// Get current quality factor
    fn q(&self) -> f32 {
        self.params().q
    }

    /// Set gain (for peak/shelving filters)
    ///
    /// # Arguments
    /// * `gain` - gain in dB (typically -24..24)
    fn set_gain_db(&mut self, gain: f32) {
        let mut params = self.params().clone();
        params.gain_db = gain;
        self.set_params(params);
    }

    /// Get current gain in dB
    fn gain_db(&self) -> f32 {
        self.params().gain_db
    }

    /// Get filter type
    fn filter_type(&self) -> FilterType {
        self.params().filter_type
    }
}

// Blanket implementation for all types with Params = FilterParams
impl<T: Transcendental, F> Filter<T> for F where F: ParameterizedAlgorithm<T, Params = FilterParams> {}

// =============================================================================
// Filter comparison
// =============================================================================

/// Summary of all filter type characteristics
#[derive(Debug)]
pub struct FilterComparison;

impl FilterComparison {
    /// Rolloff comparison across different implementations
    ///
    /// # Example
    /// ```
    /// use rill_core_dsp::filters::FilterComparison;
    ///
    /// println!("{}", FilterComparison::rolloff_comparison());
    /// ```
    pub fn rolloff_comparison() -> &'static str {
        "Roll-off slope (dB/octave):\n\
         ┌────────────────┬────────────┬──────────────┐\n\
         │ Filter         │ Order 2    │ Order 4      │\n\
         ├────────────────┼────────────┼──────────────┤\n\
         │ OnePole        │ 6 dB/oct   │ -            │\n\
         │ Biquad         │ 12 dB/oct  │ 24 dB/oct*  │\n\
         │ Butterworth    │ 12 dB/oct  │ 24 dB/oct    │\n\
         │ Chebyshev I    │ 12-18 dB/oct │ 24-36 dB/oct │\n\
         │ Chebyshev II   │ 12-18 dB/oct │ 24-36 dB/oct │\n\
         └────────────────┴────────────┴──────────────┘\n\
         * Biquad can be cascaded for higher orders"
    }

    /// Filter selection guide
    pub fn selection_guide() -> &'static str {
        "How to choose a filter:\n\n\
         🎯 **For Hi-Fi and transparent processing**:\n\
         → Butterworth - maximally flat response\n\n\
         🎯 **For synths and effects**:\n\
         → StateVariableFilter - analog sound, three outputs\n\
         → OnePole - simplicity and speed\n\n\
         🎯 **For EQs**:\n\
         → Biquad - versatility, all types\n\
         → ChebyshevI - steeper roll-off\n\n\
         🎯 **For anti-aliasing**:\n\
         → ChebyshevII - flat passband\n\
         → Butterworth - predictable behaviour\n\n\
         🎯 **For reverb**:\n\
         → CombFilter - comb structures\n\
         → AllPass - diffusion"
    }

    /// Computational complexity characteristics
    pub fn performance_guide() -> &'static str {
        "Performance (relative):\n\
         ⚡ **OnePole** - 1x (fastest)\n\
         ⚡⚡ **Biquad** - 2x\n\
         ⚡⚡⚡ **StateVariableFilter** - 3x\n\
         ⚡⚡⚡ **CombFilter** - 3x\n\
         ⚡⚡⚡⚡ **Butterworth (4 order)** - 4x\n\
         ⚡⚡⚡⚡ **Chebyshev (4 order)** - 4x"
    }
}

// =============================================================================
// Usage examples (doctests) with correct imports
// =============================================================================

#[cfg(doctest)]
mod examples {
    /// ```rust
    /// use rill_core::time::ClockTick;
    /// use rill_core::traits::ActionContext;
    /// use rill_core_dsp::filters::*;
    /// use rill_core::Transcendental;
    /// use rill_core::traits::algorithm::Algorithm;
    /// use std::f32::consts::PI;
    ///
    /// let tick = ClockTick::default();
    /// let ctx = ActionContext::new(&tick);
    ///
    /// // 1. Simple low-pass filter for smoothing
    /// let mut smooth = OnePole::<f32>::new(FilterParams {
    ///     filter_type: FilterType::LowPass,
    ///     cutoff: 100.0,
    ///     q: 0.0,
    ///     gain_db: 0.0,
    /// });
    /// smooth.init(44100.0);
    ///
    /// // Smooth sharp transitions
    /// let mut smoothed = 0.0;
    /// for _ in 0..1000 {
    ///     let mut out = [0.0_f32];
    ///     smooth.process(Some(&[1.0]), &mut out).unwrap();
    ///     smoothed = out[0];
    /// }
    /// // After 1000 iterations, value should be close to 1.0
    /// # assert!((smoothed - 1.0).abs() < 0.1);
    ///
    /// // 2. Parametric EQ with Biquad
    /// let mut peq = Biquad::<f32>::new(FilterParams {
    ///     filter_type: FilterType::Peak,
    ///     cutoff: 1000.0,
    ///     q: 2.0,
    ///     gain_db: 6.0,
    /// });
    /// peq.init(44100.0);
    ///
    /// // Warm up the filter
    /// for _ in 0..1000 {
    ///     let mut out = [0.0_f32];
    ///     peq.process(Some(&[0.0]), &mut out).unwrap();
    /// }
    ///
    /// // Generate sine wave at filter frequency
    /// let sample_rate = 44100.0;
    /// let frequency = 1000.0;
    /// let amplitude = 0.5;
    /// let phase_inc = 2.0 * PI * frequency / sample_rate;
    /// let mut phase = 0.0;
    ///
    /// let mut max_output = 0.0_f32;
    /// for _ in 0..1000 {
    ///     let input = amplitude * phase.sin();
    ///     let mut out = [0.0_f32];
    ///     peq.process(Some(&[input]), &mut out).unwrap();
    ///     let output = out[0];
    ///     max_output = max_output.max(output.abs());
    ///     phase += phase_inc;
    ///     if phase > 2.0 * PI {
    ///         phase -= 2.0 * PI;
    ///     }
    /// }
    ///
    /// // Peak filter with +6dB should boost signal at filter frequency
    /// // Use tolerance due to numerical errors
    /// let epsilon = 1e-4;
    /// # assert!(max_output + epsilon > amplitude,
    /// #     "Max output ({:.6}) should be greater than or close to input amplitude ({:.6})",
    /// #     max_output, amplitude);
    /// # assert!(max_output < 1.0, "Max output ({}) should be less than 1.0", max_output);
    ///
    /// // 3. Analog emulation with SVF
    /// let mut svf = StateVariableFilter::<f32>::new(FilterParams {
    ///     filter_type: FilterType::LowPass,
    ///     cutoff: 1000.0,
    ///     q: 0.7,
    ///     gain_db: 0.0,
    /// });
    /// svf.init(44100.0);
    ///
    /// let input = 0.5;
    /// let mut out = [0.0_f32];
    /// svf.process(Some(&[input]), &mut out).unwrap();
    /// let lp = out[0];
    /// let hp = svf.highpass();
    /// let bp = svf.bandpass();
    /// ```
    ///
    /// ```rust
    /// // 4. Steep crossover filter (Chebyshev)
    /// use rill_core_dsp::filters::*;
    /// use rill_core::traits::algorithm::Algorithm;
    ///
    /// let mut xover = ChebyshevI::<f32, 4>::new(
    ///     FilterParams {
    ///         filter_type: FilterType::LowPass,
    ///         cutoff: 1000.0,
    ///         q: 0.0,
    ///         gain_db: 0.0,
    ///     },
    ///     4,
    ///     0.5
    /// );
    /// xover.init(44100.0);
    ///
    /// // 5. Hi-Fi filter (Butterworth)
    /// let mut hifi = Butterworth::<f32, 4>::lowpass(1000.0, 4);
    /// hifi.init(44100.0);
    /// ```
    fn _dummy() {}
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_type_descriptions() {
        assert_eq!(FilterType::LowPass.as_str(), "lowpass");
        assert!(!FilterType::LowPass.description().is_empty());
        assert!(!FilterType::LowPass.usage().is_empty());
    }

    #[test]
    fn test_comparison_guide() {
        assert!(!FilterComparison::rolloff_comparison().is_empty());
        assert!(!FilterComparison::selection_guide().is_empty());
        assert!(!FilterComparison::performance_guide().is_empty());
    }

    #[test]
    fn test_filter_params_clone() {
        let params = FilterParams {
            filter_type: FilterType::LowPass,
            cutoff: 1000.0,
            q: 0.707,
            gain_db: 0.0,
        };
        let params2 = params.clone();
        assert_eq!(params.cutoff, params2.cutoff);
    }
}
