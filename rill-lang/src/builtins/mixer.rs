use rill_core::math::Transcendental;
use rill_core::traits::ProcessResult;

/// Mixer configuration extracted from the record argument.
#[derive(Debug, Clone)]
pub struct MixerConfig {
    pub num_channels: usize,
    pub num_buses: usize,
    pub channel_vols: Vec<f64>,
    pub channel_pans: Vec<f64>,
    pub channel_mutes: Vec<bool>,
    pub sends: Vec<Vec<(usize, f64, bool)>>,
    pub master_vol: f64,
    pub smoothing: f64,
}

impl MixerConfig {
    pub fn new(num_channels: usize, num_buses: usize) -> Self {
        Self {
            num_channels,
            num_buses,
            channel_vols: vec![0.8; num_channels],
            channel_pans: vec![0.0; num_channels],
            channel_mutes: vec![false; num_channels],
            sends: vec![vec![]; num_channels],
            master_vol: 1.0,
            smoothing: 0.02,
        }
    }
}

/// Runtime state for the mixer.
pub struct MixerState<T: Transcendental> {
    config: MixerConfig,
    current_vols: Vec<T>,
    current_pans: Vec<T>,
    current_master_vol: T,
    bus_buffers: Vec<Vec<T>>,
}

impl<T: Transcendental> MixerState<T> {
    pub fn new(config: MixerConfig, buf_size: usize) -> Self {
        let n_bus = config.num_buses;
        Self {
            current_vols: config
                .channel_vols
                .iter()
                .map(|&v| T::from_f64(v))
                .collect(),
            current_pans: config
                .channel_pans
                .iter()
                .map(|&v| T::from_f64(v))
                .collect(),
            current_master_vol: T::from_f64(config.master_vol),
            bus_buffers: vec![vec![T::ZERO; buf_size]; n_bus],
            config,
        }
    }

    pub fn num_inputs(&self) -> usize {
        self.config.num_channels
    }
    pub fn num_outputs(&self) -> usize {
        2 + self.config.num_buses
    }

    pub fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        let n_ch = self.config.num_channels;
        let n_bus = self.config.num_buses;
        let buf_size = outputs[0].len();
        let smoothing = T::from_f64(self.config.smoothing);

        for bus in self.bus_buffers.iter_mut() {
            bus.fill(T::ZERO);
        }

        outputs[0].fill(T::ZERO);
        if outputs.len() > 1 {
            outputs[1].fill(T::ZERO);
        }

        for sample in 0..buf_size {
            for ch in 0..n_ch {
                let input = inputs[ch][sample];

                let target_vol = T::from_f64(self.config.channel_vols[ch]);
                let target_pan = T::from_f64(self.config.channel_pans[ch]);
                self.current_vols[ch] =
                    self.current_vols[ch] + (target_vol - self.current_vols[ch]) * smoothing;
                self.current_pans[ch] =
                    self.current_pans[ch] + (target_pan - self.current_pans[ch]) * smoothing;

                if self.config.channel_mutes[ch] {
                    continue;
                }

                let vol = self.current_vols[ch];
                let pan = self.current_pans[ch];

                let (left_gain, right_gain) = if pan <= T::ZERO {
                    (T::ONE, T::ONE + pan)
                } else {
                    (T::ONE - pan, T::ONE)
                };

                let left = input * vol * left_gain;
                let right = input * vol * right_gain;

                outputs[0][sample] = outputs[0][sample] + left;
                outputs[1][sample] = outputs[1][sample] + right;

                for &(bus_idx, level, pre_fader) in &self.config.sends[ch] {
                    let send_level = T::from_f64(level);
                    if pre_fader {
                        self.bus_buffers[bus_idx][sample] =
                            self.bus_buffers[bus_idx][sample] + input * send_level;
                    } else {
                        self.bus_buffers[bus_idx][sample] =
                            self.bus_buffers[bus_idx][sample] + input * vol * send_level;
                    }
                }
            }

            let target_master = T::from_f64(self.config.master_vol);
            self.current_master_vol =
                self.current_master_vol + (target_master - self.current_master_vol) * smoothing;
            outputs[0][sample] = outputs[0][sample] * self.current_master_vol;
            outputs[1][sample] = outputs[1][sample] * self.current_master_vol;
        }

        for bus in 0..n_bus {
            outputs[2 + bus].copy_from_slice(&self.bus_buffers[bus]);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixer_silence_produces_silence() {
        let config = MixerConfig::new(2, 1);
        let mut state = MixerState::<f32>::new(config, 4);
        let inputs: &[&[f32]] = &[&[0.0; 4], &[0.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut bus0 = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r, &mut bus0];
        state.process(inputs, &mut outputs).unwrap();
        assert_eq!(out_l, [0.0; 4]);
        assert_eq!(out_r, [0.0; 4]);
        assert_eq!(bus0, [0.0; 4]);
    }

    #[test]
    fn mixer_passes_signal_at_unity() {
        let mut config = MixerConfig::new(1, 0);
        config.channel_vols = vec![1.0];
        config.smoothing = 0.0;
        let mut state = MixerState::<f32>::new(config, 4);
        let inputs: &[&[f32]] = &[&[2.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process(inputs, &mut outputs).unwrap();
        assert!((out_l[0] - 2.0).abs() < 0.001);
        assert!((out_r[0] - 2.0).abs() < 0.001);
    }

    #[test]
    fn mixer_mute_silences_channel() {
        let mut config = MixerConfig::new(1, 0);
        config.channel_mutes = vec![true];
        let mut state = MixerState::<f32>::new(config, 4);
        let inputs: &[&[f32]] = &[&[1.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process(inputs, &mut outputs).unwrap();
        assert_eq!(out_l, [0.0; 4]);
    }

    #[test]
    fn mixer_send_routes_to_bus() {
        let mut config = MixerConfig::new(1, 1);
        config.sends = vec![vec![(0, 0.5, true)]];
        config.smoothing = 0.0;
        let mut state = MixerState::<f32>::new(config, 4);
        let inputs: &[&[f32]] = &[&[2.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut bus0 = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r, &mut bus0];
        state.process(inputs, &mut outputs).unwrap();
        assert!((bus0[0] - 1.0).abs() < 0.001);
    }

    #[test]
    fn mixer_pan_full_left() {
        let mut config = MixerConfig::new(1, 0);
        config.channel_vols = vec![1.0];
        config.channel_pans = vec![-1.0];
        config.smoothing = 0.0;
        let mut state = MixerState::<f32>::new(config, 4);
        let inputs: &[&[f32]] = &[&[1.0; 4]];
        let mut out_l = [0.0f32; 4];
        let mut out_r = [0.0f32; 4];
        let mut outputs: &mut [&mut [f32]] = &mut [&mut out_l, &mut out_r];
        state.process(inputs, &mut outputs).unwrap();
        assert!((out_l[0] - 1.0).abs() < 0.001);
        assert!((out_r[0] - 0.0).abs() < 0.001);
    }
}
