use rill_core::math::Transcendental;
use rill_core::traits::ProcessResult;

/// Configuration for dry/wet signal blend.
pub struct DryWetConfig {
    /// Mix ratio: 0.0 = fully dry, 1.0 = fully wet.
    pub mix: f64,
}

/// Runtime state for dry/wet mixing (stateless — config only).
pub struct DryWetState {
    config: DryWetConfig,
}

impl DryWetState {
    /// Create a new dry/wet processor.
    pub fn new(config: DryWetConfig) -> Self {
        Self { config }
    }

    /// Number of signal inputs (2: dry, wet).
    pub fn num_inputs(&self) -> usize {
        2
    }
    /// Number of signal outputs (2: L, R — both receive the mono sum).
    pub fn num_outputs(&self) -> usize {
        2
    }

    /// Process one block: output = dry * (1 - mix) + wet * mix.
    pub fn process<T: Transcendental>(
        &self,
        inputs: &[&[T]],
        outputs: &mut [&mut [T]],
    ) -> ProcessResult<()> {
        let mix = T::from_f64(self.config.mix);
        let dry_gain = T::ONE - mix;
        let wet_gain = mix;
        let buf_size = outputs[0].len();

        for sample in 0..buf_size {
            let dry = inputs[0][sample];
            let wet = if inputs.len() > 1 {
                inputs[1][sample]
            } else {
                T::ZERO
            };

            outputs[0][sample] = dry * dry_gain + wet * wet_gain;
            outputs[1][sample] = dry * dry_gain + wet * wet_gain;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_wet_full_dry() {
        let config = DryWetConfig { mix: 0.0 };
        let state = DryWetState::new(config);
        let inputs: &[&[f32]] = &[&[1.0, 2.0, 3.0, 4.0], &[0.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process::<f32>(inputs, &mut outputs).unwrap();
        assert_eq!(out_l, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn dry_wet_full_wet() {
        let config = DryWetConfig { mix: 1.0 };
        let state = DryWetState::new(config);
        let inputs: &[&[f32]] = &[&[0.0; 4], &[1.0, 2.0, 3.0, 4.0]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process::<f32>(inputs, &mut outputs).unwrap();
        assert_eq!(out_l, [1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn dry_wet_half_mix() {
        let config = DryWetConfig { mix: 0.5 };
        let state = DryWetState::new(config);
        let inputs: &[&[f32]] = &[&[2.0; 4], &[4.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process::<f32>(inputs, &mut outputs).unwrap();
        assert!((out_l[0] - 3.0).abs() < 0.001);
    }
}
