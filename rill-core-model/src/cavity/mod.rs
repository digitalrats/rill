//! Cavity resonator models — Helmholtz single-cavity and coupled cavity arrays.
//!
//! # Sub-models
//!
//! - **HelmholtzCavity** — single Helmholtz resonator with reed excitation for
//!   wind instrument modeling.
//! - **CavityArray** — 1D chain of coupled Helmholtz cavities for wave
//!   propagation experiments (acoustic metamaterials, band gaps, dispersion).

mod params;

pub use params::{CavityArrayParams, HelmholtzParams};

use rill_core::traits::algorithm::{
    Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm,
};
use rill_core::traits::ParamValue;
use rill_core::Transcendental;

/// Single Helmholtz cavity resonator.
///
/// Models the acoustic resonance of a cavity with a neck (bottle, vessel,
/// instrument body). Implements a 2-pole bandpass filter at the Helmholtz
/// frequency:
///
/// ```text
/// f_res = c / (2π) · √(A / (V · L_eff))
/// ```
///
/// where `L_eff = L + 1.7·r` (end correction for flanged opening).
///
/// With `excitation = 1`, the reed nonlinearity drives self-oscillation
/// for wind instrument simulation (clarinet/saxophone-like behavior).
#[derive(Debug, Clone)]
pub struct HelmholtzCavity<T: Transcendental> {
    params: HelmholtzParams<T>,
    prev_out: T,
    prev_prev_out: T,
    reed_state: T,
    r: T,
    cos_omega: T,
    sample_rate: f32,
}

impl<T: Transcendental> HelmholtzCavity<T> {
    /// Create a Helmholtz cavity resonator.
    pub fn new(params: HelmholtzParams<T>, sample_rate: f32) -> Self {
        let mut cavity = Self {
            params,
            prev_out: T::ZERO,
            prev_prev_out: T::ZERO,
            reed_state: T::ZERO,
            r: T::ONE,
            cos_omega: T::ONE,
            sample_rate,
        };
        cavity.recompute_coeffs();
        cavity
    }

    /// Compute the Helmholtz resonant frequency in Hz.
    pub fn resonant_frequency(&self) -> T {
        let two_pi = T::from_f32(2.0 * std::f32::consts::PI);
        let radius = (self.params.neck_area / T::PI).sqrt();
        let l_eff = self.params.neck_length + T::from_f32(1.7) * radius;
        self.params.sound_speed / two_pi
            * (self.params.neck_area / (self.params.volume * l_eff)).sqrt()
    }

    fn recompute_coeffs(&mut self) {
        if self.sample_rate == 0.0 {
            return;
        }
        let sr = T::from_f32(self.sample_rate);
        let f_res = self.resonant_frequency();
        let omega = T::from_f32(2.0 * std::f32::consts::PI) * f_res / sr;
        let damping = T::ONE - self.params.radiation_loss - self.params.viscous_loss;
        self.r = damping;
        self.cos_omega = omega.cos();
    }

    /// Reed nonlinearity — simplified single-reed model.
    fn reed_flow(&mut self) -> T {
        let closing = T::ONE - self.params.pressure - self.reed_state;
        if closing > T::ZERO {
            self.params.pressure * closing.sqrt()
        } else {
            T::ZERO
        }
    }

    fn process_sample(&mut self, input: T) -> T {
        if self.sample_rate == 0.0 {
            return T::ZERO;
        }
        let excitation = if self.params.excitation == 1 {
            let flow = self.reed_flow();
            self.reed_state = flow;
            flow
        } else {
            input
        };

        // 2-pole bandpass resonator (center frequency = Helmholtz freq)
        let two_r_cos = T::from_f32(2.0) * self.r * self.cos_omega;
        let r2 = self.r * self.r;
        let y = excitation + two_r_cos * self.prev_out - r2 * self.prev_prev_out;

        self.prev_prev_out = self.prev_out;
        self.prev_out = y;

        y
    }
}

impl<T: Transcendental> Algorithm<T> for HelmholtzCavity<T> {
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
        self.prev_out = T::ZERO;
        self.prev_prev_out = T::ZERO;
        self.reed_state = T::ZERO;
        self.recompute_coeffs();
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.recompute_coeffs();
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Helmholtz Cavity",
            category: AlgorithmCategory::Filter,
            description: "Single Helmholtz resonator with optional reed excitation",
            author: "Rill",
            version: "0.5",
        }
    }
}

impl<T: Transcendental> ParameterizedAlgorithm<T> for HelmholtzCavity<T> {
    type Params = HelmholtzParams<T>;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.recompute_coeffs();
    }

    fn set_parameter(&mut self, name: &str, value: ParamValue) -> Result<(), &'static str> {
        match name {
            "volume" => {
                let mut p = self.params.clone();
                p.volume = T::from_f64(value.as_f32().map(|v| v as f64).unwrap_or(0.001));
                self.set_params(p);
                Ok(())
            }
            "neck_area" => {
                let mut p = self.params.clone();
                p.neck_area = T::from_f64(value.as_f32().map(|v| v as f64).unwrap_or(0.0001));
                self.set_params(p);
                Ok(())
            }
            "neck_length" => {
                let mut p = self.params.clone();
                p.neck_length = T::from_f64(value.as_f32().map(|v| v as f64).unwrap_or(0.02));
                self.set_params(p);
                Ok(())
            }
            "pressure" => {
                let mut p = self.params.clone();
                p.pressure = T::from_f64(value.as_f32().map(|v| v as f64).unwrap_or(0.0));
                self.set_params(p);
                Ok(())
            }
            "excitation" => {
                let mut p = self.params.clone();
                p.excitation = value.as_i32().map(|v| v as u8).unwrap_or(0);
                self.set_params(p);
                Ok(())
            }
            _ => Err("Unknown parameter"),
        }
    }
}

/// 1D array of N coupled Helmholtz cavities.
///
/// Each cavity is coupled to its nearest neighbors with strength `coupling`.
/// The array supports wave propagation experiments — injecting a signal at
/// `input_index` and measuring at `output_index` reveals acoustic band gaps,
/// slow-wave propagation, and nonlinear dispersion effects.
#[derive(Debug, Clone)]
pub struct CavityArray<T: Transcendental, const MAX_CAVITIES: usize> {
    params: CavityArrayParams<T>,
    cavities: [HelmholtzCavity<T>; MAX_CAVITIES],
    prev_outputs: [T; MAX_CAVITIES],
}

impl<T: Transcendental, const MAX_CAVITIES: usize> CavityArray<T, MAX_CAVITIES> {
    /// Create a cavity array with N identical Helmholtz cavities.
    pub fn new(params: CavityArrayParams<T>, sample_rate: f32) -> Self {
        let default_cavity = HelmholtzCavity::new(params.cavity_params.clone(), sample_rate);
        let cavities = core::array::from_fn(|_| default_cavity.clone());
        Self {
            params,
            cavities,
            prev_outputs: [T::ZERO; MAX_CAVITIES],
        }
    }

    fn process_sample(&mut self, input: T) -> T {
        let n = self.params.num_cavities.min(MAX_CAVITIES);

        // Store current outputs for coupling
        let prev = self.prev_outputs;

        let mut output = T::ZERO;
        for i in 0..n {
            // Coupled input: nearest-neighbor coupling
            let coupling_input = if i > 0 {
                prev[i - 1] * self.params.coupling
            } else {
                T::ZERO
            } + if i + 1 < n {
                prev[i + 1] * self.params.coupling
            } else {
                T::ZERO
            };

            // External input at the designated position
            let ext_input = if i == self.params.input_index {
                input
            } else {
                T::ZERO
            };

            let y = self.cavities[i].process_sample(coupling_input + ext_input);
            self.prev_outputs[i] = y;

            if i == self.params.output_index {
                output = y;
            }
        }

        output
    }
}

impl<T: Transcendental, const MAX_CAVITIES: usize> Algorithm<T> for CavityArray<T, MAX_CAVITIES> {
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
        for cavity in self.cavities.iter_mut() {
            cavity.reset();
        }
        self.prev_outputs = [T::ZERO; MAX_CAVITIES];
    }

    fn init(&mut self, sample_rate: f32) {
        for cavity in self.cavities.iter_mut() {
            cavity.init(sample_rate);
        }
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Cavity Array",
            category: AlgorithmCategory::Filter,
            description: "1D chain of coupled Helmholtz cavities for wave propagation",
            author: "Rill",
            version: "0.5",
        }
    }
}

impl<T: Transcendental, const MAX_CAVITIES: usize> ParameterizedAlgorithm<T>
    for CavityArray<T, MAX_CAVITIES>
{
    type Params = CavityArrayParams<T>;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        for cavity in self.cavities.iter_mut() {
            cavity.set_params(self.params.cavity_params.clone());
        }
    }

    fn set_parameter(&mut self, name: &str, value: ParamValue) -> Result<(), &'static str> {
        match name {
            "coupling" => {
                let mut p = self.params.clone();
                p.coupling = T::from_f64(value.as_f32().map(|v| v as f64).unwrap_or(0.1));
                self.set_params(p);
                Ok(())
            }
            _ => Err("Unknown parameter: use HelmholtzCavity for per-cavity params"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- HelmholtzCavity tests ---

    #[test]
    fn test_helmholtz_creation() {
        let params = HelmholtzParams::<f64>::default();
        let cavity = HelmholtzCavity::<f64>::new(params, 44100.0);
        let f = cavity.resonant_frequency();
        assert!(f.to_f64() > 0.0);
        assert!(f.to_f64() < 44100.0 / 2.0);
    }

    #[test]
    fn test_helmholtz_frequency_known_bottle() {
        // 1L bottle with 2cm neck, 1 cm² area → ~120 Hz resonance
        let params = HelmholtzParams {
            volume: 0.001,
            neck_area: 0.0001,
            neck_length: 0.02,
            sound_speed: 343.0,
            ..Default::default()
        };
        let cavity = HelmholtzCavity::<f64>::new(params, 44100.0);
        let f = cavity.resonant_frequency().to_f64();
        assert!(f > 50.0);
        assert!(f < 300.0);
    }

    #[test]
    fn test_helmholtz_algorithm_process() {
        // Feed a sine at the resonance frequency — should pass through
        let params = HelmholtzParams::<f64>::default();
        let mut cavity = HelmholtzCavity::<f64>::new(params.clone(), 44100.0);
        let f_res = cavity.resonant_frequency().to_f64();
        let mut output = [0.0f64; 128];
        let input: Vec<f64> = (0..128)
            .map(|i| (2.0 * std::f64::consts::PI * f_res * i as f64 / 44100.0).sin() * 0.5)
            .collect();
        cavity.process(Some(&input), &mut output).unwrap();
        let rms = (output.iter().map(|x| x * x).sum::<f64>() / 128.0).sqrt();
        assert!(
            rms > 0.01,
            "RMS should be non-zero at resonance: {:.6}",
            rms
        );
    }

    #[test]
    fn test_helmholtz_reed_self_oscillation() {
        // With pressure and excitation=1, should produce non-zero output
        let params = HelmholtzParams {
            pressure: 0.5,
            excitation: 1,
            ..Default::default()
        };
        let mut cavity = HelmholtzCavity::<f64>::new(params, 44100.0);
        let mut output = [0.0f64; 128];
        cavity.process(None, &mut output).unwrap();
        let max_abs = output.iter().map(|x| x.abs()).fold(0.0, f64::max);
        assert!(max_abs > 0.0, "Reed excitation should produce output");
    }

    #[test]
    fn test_helmholtz_params() {
        let params = HelmholtzParams::<f64>::default();
        let mut cavity = HelmholtzCavity::<f64>::new(params, 44100.0);
        let new_params = HelmholtzParams {
            volume: 0.002,
            ..HelmholtzParams::default()
        };
        cavity.set_params(new_params);
        assert!((cavity.params.volume.to_f64() - 0.002).abs() < 1e-10);
    }

    // --- CavityArray tests ---

    #[test]
    fn test_cavity_array_creation() {
        let params = CavityArrayParams::<f64>::default();
        let array = CavityArray::<f64, 8>::new(params, 44100.0);
        assert!(array.params.num_cavities == 4);
    }

    #[test]
    fn test_cavity_array_wave_propagation() {
        // Input at cavity 0, output at cavity 3 with coupling — should see signal
        let params = CavityArrayParams {
            num_cavities: 4,
            coupling: 0.3,
            input_index: 0,
            output_index: 3,
            ..Default::default()
        };
        let mut array = CavityArray::<f64, 8>::new(params, 44100.0);
        let mut output = [0.0f64; 256];
        let input: Vec<f64> = (0..256)
            .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 44100.0).sin() * 0.5)
            .collect();
        array.process(Some(&input), &mut output).unwrap();
        let rms = (output.iter().map(|x| x * x).sum::<f64>() / 256.0).sqrt();
        assert!(
            rms > 0.001,
            "Wave should propagate through coupled cavities"
        );
    }

    #[test]
    fn test_cavity_array_zero_coupling() {
        // Zero coupling — no propagation, output should be near zero
        let params = CavityArrayParams {
            num_cavities: 4,
            coupling: 0.0,
            input_index: 0,
            output_index: 3,
            ..Default::default()
        };
        let mut array = CavityArray::<f64, 8>::new(params, 44100.0);
        let mut output = [0.0f64; 256];
        let input: Vec<f64> = (0..256)
            .map(|i| (2.0 * std::f64::consts::PI * 440.0 * i as f64 / 44100.0).sin() * 0.5)
            .collect();
        array.process(Some(&input), &mut output).unwrap();
        let rms = (output.iter().map(|x| x * x).sum::<f64>() / 256.0).sqrt();
        assert!(rms < 0.01, "Zero coupling should block propagation");
    }
}
