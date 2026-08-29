use rill_core::builtin::BlockBuiltin;
use rill_core::math::Transcendental;
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::ProcessResult;

use crate::lang::pv_f32;

/// Running-sum integrator: `out[n] = x[n] + out[n-1]`.
pub struct IntegratorBuiltin<T: Transcendental> {
    state: T,
}

impl<T: Transcendental> IntegratorBuiltin<T> {
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
