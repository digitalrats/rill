//! Builtin wrapper structs for the rill-core-dsp filters.
//!
//! Moved from `rill-core-dsp/src/lang` (SP-3b Task 6, ahead of Task 8): these
//! adapt `Biquad`, `OnePole`, and `MoogLadder` to the DSL's `BlockBuiltin`
//! contract. `lowpass`/`highpass` are `biquad` with a preset filter type.

use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ProcessResult;
use rill_core_dsp::algorithm::ParameterizedAlgorithm;
use rill_core_dsp::filters::{Biquad, Filter, FilterType, MoogLadder, OnePole};

use super::pv_f32;
use crate::builtin::BlockBuiltin;

/// A `BlockBuiltin` wrapping a [`Biquad`] preset to one filter type.
pub struct BiquadBuiltin<T: Transcendental> {
    /// The wrapped biquad filter.
    pub inner: Biquad<T>,
}

impl<T: Transcendental> Algorithm<T> for BiquadBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.inner.process(input, output)
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
    fn init(&mut self, sample_rate: f32) {
        Algorithm::init(&mut self.inner, sample_rate);
    }
    fn apply_command(&mut self, value: T) {
        Algorithm::apply_command(&mut self.inner, value);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for BiquadBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => Filter::set_cutoff(&mut self.inner, v),
            1 => Filter::set_q(&mut self.inner, v),
            _ => {}
        }
    }
}

/// A `BlockBuiltin` wrapping a [`Biquad`] whose filter type is a parameter
/// (the `biquad` builtin's `type`/`cutoff`/`q`/`gain_db`).
pub struct GeneralBiquadBuiltin<T: Transcendental> {
    /// The wrapped biquad filter.
    pub inner: Biquad<T>,
}

impl<T: Transcendental> Algorithm<T> for GeneralBiquadBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.inner.process(input, output)
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
    fn init(&mut self, sample_rate: f32) {
        Algorithm::init(&mut self.inner, sample_rate);
    }
    fn apply_command(&mut self, value: T) {
        Algorithm::apply_command(&mut self.inner, value);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for GeneralBiquadBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => {
                let ft = match v as u8 {
                    0 => FilterType::LowPass,
                    1 => FilterType::HighPass,
                    2 => FilterType::BandPass,
                    3 => FilterType::Notch,
                    4 => FilterType::Peak,
                    5 => FilterType::LowShelf,
                    6 => FilterType::HighShelf,
                    _ => FilterType::LowPass,
                };
                let mut params = self.inner.params().clone();
                params.filter_type = ft;
                self.inner.set_params(params);
            }
            1 => Filter::set_cutoff(&mut self.inner, v),
            2 => Filter::set_q(&mut self.inner, v),
            3 => Filter::set_gain_db(&mut self.inner, v),
            _ => {}
        }
    }
}

/// A `BlockBuiltin` wrapping a [`OnePole`]: the `onepole` lowpass filter.
pub struct OnePoleBuiltin<T: Transcendental> {
    /// The wrapped one-pole filter.
    pub inner: OnePole<T>,
}

impl<T: Transcendental> Algorithm<T> for OnePoleBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        Algorithm::process(&mut self.inner, input, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.inner, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for OnePoleBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => Filter::set_cutoff(&mut self.inner, v),
            1 => Filter::set_q(&mut self.inner, v),
            _ => {}
        }
    }
}

/// A `BlockBuiltin` wrapping a [`MoogLadder`]: the `moog` filter.
pub struct MoogBuiltin<T: Transcendental> {
    /// The wrapped Moog ladder filter.
    pub inner: MoogLadder<T>,
}

impl<T: Transcendental> Algorithm<T> for MoogBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        Algorithm::process(&mut self.inner, input, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.inner, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.inner);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for MoogBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        let v = pv_f32(value);
        match index {
            0 => self.inner.set_cutoff(v),
            1 => self.inner.set_resonance(v),
            _ => {}
        }
    }
}
