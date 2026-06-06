//! Modal resonator — parallel bank of 2-pole filters.
//!
//! Implements modal synthesis: an object is modeled as a sum of N
//! exponentially decaying sinusoidal modes, each represented by a
//! 2-pole resonator `H(z) = 1 / (1 - 2r·cos(ω)·z⁻¹ + r²·z⁻²)`.
//! Excitation is an impulse at the fundamental frequency.

mod params;

pub use params::{bell_modes, marimba_modes, ModalParams, ModeParams};

use rill_core::traits::algorithm::{
    Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm,
};
use rill_core::traits::ParamValue;
use rill_core::Transcendental;

/// Internal state for one resonant mode.
#[derive(Debug, Clone, Copy)]
struct ModeState<T: Transcendental> {
    prev_out: T,
    prev_prev_out: T,
    r: T,
    cos_omega: T,
    amplitude: T,
}

impl<T: Transcendental> Default for ModeState<T> {
    fn default() -> Self {
        Self {
            prev_out: T::ZERO,
            prev_prev_out: T::ZERO,
            r: T::from_f32(0.99),
            cos_omega: T::ONE,
            amplitude: T::ZERO,
        }
    }
}

/// Modal resonator — parallel resonant filter bank.
///
/// Pre-allocates `MAX_MODES` mode states at construction. The `process()`
/// method evaluates all active modes in parallel — RT-safe, no allocation.
#[derive(Debug, Clone)]
pub struct ModalModel<T: Transcendental, const MAX_MODES: usize> {
    params: ModalParams<T, MAX_MODES>,
    mode_states: [ModeState<T>; MAX_MODES],
    excitation: T,
    sample_rate: f32,
}

impl<T: Transcendental, const MAX_MODES: usize> ModalModel<T, MAX_MODES> {
    /// Create a modal model with the given parameters.
    pub fn new(params: ModalParams<T, MAX_MODES>, sample_rate: f32) -> Self {
        let mut model = Self {
            params,
            mode_states: [ModeState::default(); MAX_MODES],
            excitation: T::ZERO,
            sample_rate,
        };
        model.recompute_coeffs();
        model
    }

    /// Excite the resonator (strike, pluck, hammer).
    pub fn strike(&mut self, strength: T) {
        self.excitation = strength;
    }

    fn recompute_coeffs(&mut self) {
        let sr = T::from_f32(self.sample_rate);
        let two_pi = T::from_f32(2.0 * std::f32::consts::PI);
        for i in 0..self.params.num_modes.min(MAX_MODES) {
            let mode = &self.params.modes[i];
            let freq = mode.freq_ratio * self.params.fundamental;
            let decay = mode.decay_time * self.params.damping;
            let omega = two_pi * freq / sr;
            let r = if decay > T::ZERO {
                (-T::ONE / (decay * sr)).exp()
            } else {
                T::from_f32(0.999)
            };
            self.mode_states[i] = ModeState {
                r,
                cos_omega: omega.cos(),
                amplitude: mode.amplitude,
                ..self.mode_states[i]
            };
        }
    }

    fn process_sample(&mut self, _input: T) -> T {
        if self.sample_rate == 0.0 {
            return T::ZERO;
        }
        let mut output = T::ZERO;
        let active = self.params.num_modes.min(MAX_MODES);
        for i in 0..active {
            let state = &mut self.mode_states[i];
            let two_r_cos = T::from_f32(2.0) * state.r * state.cos_omega;
            let r2 = state.r * state.r;
            let y = self.excitation * state.amplitude + two_r_cos * state.prev_out
                - r2 * state.prev_prev_out;
            output = output + y;
            state.prev_prev_out = state.prev_out;
            state.prev_out = y;
        }
        self.excitation = self.excitation * T::from_f32(0.99);
        output
    }
}

impl<T: Transcendental, const MAX_MODES: usize> Algorithm<T> for ModalModel<T, MAX_MODES> {
    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
    ) -> rill_core::traits::ProcessResult<()> {
        for (i, out) in output.iter_mut().enumerate() {
            let inp = input
                .map(|s| s.get(i).copied().unwrap_or(T::ZERO))
                .unwrap_or(T::ZERO);
            *out = self.process_sample(inp);
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.mode_states = [ModeState::default(); MAX_MODES];
        self.excitation = T::ZERO;
        self.recompute_coeffs();
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.recompute_coeffs();
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Modal Resonator",
            category: AlgorithmCategory::Generator,
            description: "Parallel resonant filter bank for modal synthesis",
            author: "Rill",
            version: "0.5",
        }
    }
}

impl<T: Transcendental, const MAX_MODES: usize> ParameterizedAlgorithm<T>
    for ModalModel<T, MAX_MODES>
{
    type Params = ModalParams<T, MAX_MODES>;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.recompute_coeffs();
    }

    fn set_parameter(&mut self, name: &str, value: ParamValue) -> Result<(), &'static str> {
        match name {
            "fundamental" => {
                let mut p = self.params.clone();
                p.fundamental = T::from_f32(value.as_f32().unwrap_or(440.0));
                self.set_params(p);
                Ok(())
            }
            "damping" => {
                let mut p = self.params.clone();
                p.damping = T::from_f32(value.as_f32().unwrap_or(1.0));
                self.set_params(p);
                Ok(())
            }
            _ => Err("Unknown parameter"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_creation() {
        let params = ModalParams::<f64, 8>::default();
        let model = ModalModel::<f64, 8>::new(params, 44100.0);
        assert_eq!(model.params.num_modes, 1);
    }

    #[test]
    fn test_modal_algorithm_process() {
        let params = ModalParams::<f64, 8>::default();
        let mut model = ModalModel::<f64, 8>::new(params, 44100.0);
        model.strike(1.0.into());
        let mut output = [0.0f64; 64];
        model.process(None, &mut output).unwrap();
        let max_abs = output.iter().map(|x| x.abs()).fold(0.0, f64::max);
        assert!(max_abs > 0.0);
    }

    #[test]
    fn test_modal_decay() {
        let params = ModalParams::<f64, 8> {
            modes: {
                let arr = [ModeParams {
                    freq_ratio: 1.0.into(),
                    amplitude: 1.0.into(),
                    decay_time: 0.002.into(),
                }; 8];
                arr
            },
            num_modes: 1,
            fundamental: 440.0.into(),
            damping: 1.0.into(),
        };
        let mut model = ModalModel::<f64, 8>::new(params, 44100.0);
        model.strike(1.0.into());
        let mut blocks = Vec::new();
        for _ in 0..10 {
            let mut out = [0.0f64; 64];
            model.process(None, &mut out).unwrap();
            blocks.push(out.iter().map(|x| x.abs()).fold(0.0, f64::max));
        }
        assert!(blocks[9] < blocks[0] * 0.1);
    }

    #[test]
    fn test_bell_modes() {
        let params: ModalParams<f64, 8> = bell_modes();
        let model = ModalModel::<f64, 8>::new(params, 44100.0);
        assert_eq!(model.params.num_modes, 5);
    }

    #[test]
    fn test_marimba_modes() {
        let params: ModalParams<f64, 8> = marimba_modes();
        let model = ModalModel::<f64, 8>::new(params, 44100.0);
        assert_eq!(model.params.num_modes, 3);
    }

    #[test]
    fn test_modal_params() {
        let params = ModalParams::<f64, 8>::default();
        let mut model = ModalModel::<f64, 8>::new(params.clone(), 44100.0);
        let mut new_params = params.clone();
        new_params.fundamental = 220.0.into();
        model.set_params(new_params);
        assert!((model.params.fundamental - 220.0).abs() < 1e-10);
    }
}
