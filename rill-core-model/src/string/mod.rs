//! Physical string model — digital waveguide with stiffness and damping.
//!
//! Implements a 1D waveguide using a delay line with fractional-delay allpass
//! interpolation, a loop filter for frequency-dependent damping, and an
//! allpass dispersion filter for inharmonic stiff-string behavior.

mod params;

pub use params::StringParams;

use rill_core::traits::algorithm::{
    Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm,
};
use rill_core::traits::ParamValue;
use rill_core::Transcendental;

/// Physical string model — digital waveguide with damping and dispersion.
///
/// Uses a pre-allocated delay line (`Vec<T>`) sized to the maximum supported
/// delay. The `process()` method is RT-safe — no allocation, no locking.
#[derive(Debug, Clone)]
pub struct StringModel<T: Transcendental> {
    params: StringParams<T>,
    delay_line: Vec<T>,
    write_ptr: usize,
    delay_len: usize,
    frac: T,
    prev_allpass: T,
    prev_input: T,
    sample_rate: f32,
}

impl<T: Transcendental> StringModel<T> {
    /// Create a string model with the given parameters and delay-line capacity.
    ///
    /// `capacity` samples should exceed `sample_rate / min_frequency`.
    pub fn new(params: StringParams<T>, sample_rate: f32, capacity: usize) -> Self {
        let delay_len = (sample_rate as f64 / params.frequency.to_f64()) as usize;
        let delay_len = delay_len.min(capacity).max(2);
        let frac = T::from_f64(sample_rate as f64 / params.frequency.to_f64() - delay_len as f64);
        Self {
            params,
            delay_line: vec![T::ZERO; capacity],
            write_ptr: 0,
            delay_len,
            frac,
            prev_allpass: T::ZERO,
            prev_input: T::ZERO,
            sample_rate,
        }
    }

    /// Excite the string with an impulse (pluck).
    pub fn pluck(&mut self, strength: T) {
        let two = T::from_f32(2.0);
        let half = T::from_f32(0.5);
        for i in 0..self.delay_len.min(self.delay_line.len()) {
            // Write backward from write_ptr so process_sample reads from filled region
            let pos = (self.write_ptr + self.delay_line.len() - 1 - i) % self.delay_line.len();
            let phase = T::from_f64(i as f64 / self.delay_len as f64);
            let noise = (T::random() - half) * two * strength;
            self.delay_line[pos] = noise * (T::ONE - phase);
        }
    }

    /// Excite the string with a shaped excitation buffer (bow, hammer, etc.).
    pub fn excite(&mut self, excitation: &[T]) {
        for (i, &sample) in excitation.iter().enumerate() {
            let pos = (self.write_ptr + i) % self.delay_line.len();
            self.delay_line[pos] = sample;
        }
    }

    fn process_sample(&mut self, input: T) -> T {
        let read_ptr =
            (self.write_ptr + self.delay_line.len() - self.delay_len) % self.delay_line.len();
        let read_next = (read_ptr + 1) % self.delay_line.len();

        let s0 = self.delay_line[read_ptr];
        let s1 = self.delay_line[read_next];

        // Fractional-delay allpass interpolation
        let c = (T::ONE - self.frac) / (T::ONE + self.frac);
        let delayed = -c * s0 + s1 + c * self.prev_allpass;
        self.prev_allpass = delayed;

        // Loop filter: one-pole lowpass for brightness control
        let b = self.params.brightness;
        let filtered = (T::ONE - b) * self.prev_input + b * delayed;

        // Allpass dispersion filter for stiffness (inharmonicity)
        let stiff = self.params.stiffness;
        let dispersed = if stiff > T::ZERO {
            -stiff * filtered + self.prev_input + stiff * self.prev_input
        } else {
            filtered
        };
        self.prev_input = filtered;

        let output = dispersed * self.params.decay + input;

        self.delay_line[self.write_ptr] = output;
        self.write_ptr = (self.write_ptr + 1) % self.delay_line.len();

        output
    }
}

impl<T: Transcendental> Algorithm<T> for StringModel<T> {
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
        self.delay_line.fill(T::ZERO);
        self.write_ptr = 0;
        self.prev_allpass = T::ZERO;
        self.prev_input = T::ZERO;
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let delay_len = (sample_rate as f64 / self.params.frequency.to_f64()) as usize;
        self.delay_len = delay_len.min(self.delay_line.len()).max(2);
        self.frac =
            T::from_f64(sample_rate as f64 / self.params.frequency.to_f64() - delay_len as f64);
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "String Model",
            category: AlgorithmCategory::Generator,
            description: "1D digital waveguide with stiffness, damping, and brightness control",
            author: "Rill",
            version: "0.5",
        }
    }
}

impl<T: Transcendental> ParameterizedAlgorithm<T> for StringModel<T> {
    type Params = StringParams<T>;

    fn params(&self) -> &Self::Params {
        &self.params
    }

    fn set_params(&mut self, params: Self::Params) {
        let freq_changed = params.frequency != self.params.frequency;
        self.params = params;
        if freq_changed && self.sample_rate > 0.0 {
            let delay_len = (self.sample_rate as f64 / self.params.frequency.to_f64()) as usize;
            self.delay_len = delay_len.min(self.delay_line.len()).max(2);
            self.frac = T::from_f64(
                self.sample_rate as f64 / self.params.frequency.to_f64() - delay_len as f64,
            );
        }
    }

    fn set_parameter(&mut self, name: &str, value: ParamValue) -> Result<(), &'static str> {
        match name {
            "frequency" => {
                let mut p = self.params.clone();
                p.frequency = T::from_f32(value.as_f32().unwrap_or(440.0));
                self.set_params(p);
                Ok(())
            }
            "decay" => {
                let mut p = self.params.clone();
                p.decay = T::from_f32(value.as_f32().unwrap_or(0.9995));
                self.set_params(p);
                Ok(())
            }
            "stiffness" => {
                let mut p = self.params.clone();
                p.stiffness = T::from_f32(value.as_f32().unwrap_or(0.0));
                self.set_params(p);
                Ok(())
            }
            "brightness" => {
                let mut p = self.params.clone();
                p.brightness = T::from_f32(value.as_f32().unwrap_or(0.95));
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
    fn test_string_creation() {
        let params = StringParams::default();
        let model = StringModel::<f64>::new(params, 44100.0, 4096);
        assert!(model.delay_len >= 2);
        assert!(model.delay_len <= 4096);
    }

    #[test]
    fn test_string_algorithm_process() {
        let params = StringParams::default();
        let mut model = StringModel::<f64>::new(params, 44100.0, 4096);
        model.pluck(0.5.into());
        let mut output = [0.0f64; 64];
        model.process(None, &mut output).unwrap();
        let max_abs = output.iter().map(|x| x.abs()).fold(0.0, f64::max);
        assert!(max_abs > 0.0);
    }

    #[test]
    fn test_string_decay() {
        let params = StringParams {
            decay: 0.5.into(),
            ..Default::default()
        };
        let mut model = StringModel::<f64>::new(params, 44100.0, 4096);
        model.pluck(1.0.into());
        let mut blocks = Vec::new();
        for _ in 0..20 {
            let mut out = [0.0f64; 64];
            model.process(None, &mut out).unwrap();
            blocks.push(out.iter().map(|x| x.abs()).fold(0.0, f64::max));
        }
        // Signal should decay over time
        assert!(blocks[19] < blocks[0] * 0.5);
    }

    #[test]
    fn test_string_params() {
        let params = StringParams::default();
        let mut model = StringModel::<f64>::new(params, 44100.0, 4096);
        let new_params = StringParams {
            frequency: 220.0.into(),
            ..StringParams::default()
        };
        model.set_params(new_params);
        assert!((model.params.frequency - 220.0).abs() < 1e-6);
    }

    #[test]
    fn test_string_set_parameter() {
        let params = StringParams::default();
        let mut model = StringModel::<f64>::new(params, 44100.0, 4096);
        model
            .set_parameter("frequency", ParamValue::Float(220.0))
            .unwrap();
        assert!((model.params.frequency - 220.0).abs() < 1e-6);
        assert!(model
            .set_parameter("unknown", ParamValue::Float(1.0))
            .is_err());
    }
}
