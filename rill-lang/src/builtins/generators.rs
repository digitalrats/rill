//! Builtin wrapper structs for the rill-core-dsp generators and integrators.
//!
//! Moved from `rill-core-dsp/src/lang` (SP-3b Task 6): rill-core-dsp is a pure
//! algorithms library, so these adapt its `BasicOscillator`, `NoiseGenerator`,
//! and running-sum integrators to the DSL's `BlockBuiltin` contract here in the
//! language crate.

use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ProcessResult;
use rill_core_dsp::generators::{BasicOscillator, Generator, NoiseGenerator};

use super::pv_f32;
use crate::builtin::BlockBuiltin;

/// A `BlockBuiltin` wrapping a [`BasicOscillator`]: a 0-input signal generator
/// for the `sine`/`saw`/`square`/`triangle` builtins.
pub struct OscBuiltin<T: Transcendental> {
    /// The wrapped oscillator algorithm.
    pub osc: BasicOscillator<T>,
}

impl<T: Transcendental> Algorithm<T> for OscBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.osc.process(input, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.osc, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.osc);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for OscBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        match index {
            0 => self.osc.set_frequency(pv_f32(value)),
            1 => self.osc.set_amplitude(T::from_f32(pv_f32(value))),
            2 => self.osc.set_phase(T::from_f32(pv_f32(value))),
            _ => {}
        }
    }
}

/// A `BlockBuiltin` wrapping a [`NoiseGenerator`]: the 0-input `noise` builtin.
pub struct NoiseGenBuiltin<T: Transcendental> {
    /// The wrapped noise generator algorithm.
    pub gen: NoiseGenerator<T>,
}

impl<T: Transcendental> Algorithm<T> for NoiseGenBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        self.gen.process(input, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.gen, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.gen);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for NoiseGenBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        if index == 1 {
            self.gen.set_amplitude(T::from_f32(pv_f32(value)));
        }
    }
}

/// Running-sum integrator: `out[n] = x[n] + out[n-1]`.
pub struct IntegratorBuiltin<T: Transcendental> {
    state: T,
}

impl<T: Transcendental> IntegratorBuiltin<T> {
    /// A fresh integrator with a zero running sum.
    pub fn new() -> Self {
        Self { state: T::ZERO }
    }
}

impl<T: Transcendental> Default for IntegratorBuiltin<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental> Algorithm<T> for IntegratorBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len());
                for (o, &x) in output[..n].iter_mut().zip(inp[..n].iter()) {
                    self.state += x;
                    *o = self.state;
                }
                output[n..].fill(T::ZERO);
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.state = T::ZERO;
    }
}

impl<T: Transcendental> BlockBuiltin<T> for IntegratorBuiltin<T> {}

/// Leaky integrator: `out[n] = x[n] + coeff * out[n-1]`.
pub struct LeakyIntegratorBuiltin<T: Transcendental> {
    state: T,
    coeff: T,
}

impl<T: Transcendental> LeakyIntegratorBuiltin<T> {
    /// A leaky integrator with the given leak coefficient and a zero state.
    pub fn new(coeff: T) -> Self {
        Self {
            state: T::ZERO,
            coeff,
        }
    }
}

impl<T: Transcendental> Algorithm<T> for LeakyIntegratorBuiltin<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                let n = inp.len().min(output.len());
                for (o, &x) in output[..n].iter_mut().zip(inp[..n].iter()) {
                    self.state = x + self.coeff * self.state;
                    *o = self.state;
                }
                output[n..].fill(T::ZERO);
            }
            None => output.fill(T::ZERO),
        }
        Ok(())
    }
    fn reset(&mut self) {
        self.state = T::ZERO;
    }
}

impl<T: Transcendental> BlockBuiltin<T> for LeakyIntegratorBuiltin<T> {
    fn set_param(&mut self, index: usize, value: &rill_core::traits::ParamValue) {
        if index == 0 {
            self.coeff = T::from_f32(pv_f32(value));
        }
    }
}
