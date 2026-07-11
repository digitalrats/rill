use rill_core::traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use rill_core::traits::ProcessResult;

use crate::config::LofiConfig;

/// Emulates the Akai S900 hardware sampler — 12-bit sample playback
/// with linear interpolation, pitch shifting and loop support.
pub struct AkaiS900Emulator<const BUF_SIZE: usize> {
    buffer: Vec<f32>,
    position: f32,
    pitch: f32,
    loop_enabled: bool,
    loop_start: usize,
    loop_end: usize,
    config: LofiConfig,
}

impl<const BUF_SIZE: usize> Default for AkaiS900Emulator<BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const BUF_SIZE: usize> AkaiS900Emulator<BUF_SIZE> {
    pub fn new() -> Self {
        let config = LofiConfig::for_system(crate::config::ClassicSystem::AkaiS900);
        Self {
            buffer: Vec::new(),
            position: 0.0,
            pitch: 1.0,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            config,
        }
    }

    pub fn load_sample(&mut self, samples: &[f32]) {
        self.buffer = samples.to_vec();
        self.loop_end = samples.len();
    }

    pub fn set_pitch(&mut self, pitch: f32) {
        self.pitch = pitch.clamp(0.1, 4.0);
    }

    pub fn set_loop(&mut self, enabled: bool, start: usize, end: usize) {
        self.loop_enabled = enabled;
        self.loop_start = start;
        self.loop_end = end.min(self.buffer.len());
    }

    fn generate_sample(&mut self) -> f32 {
        if self.buffer.is_empty() || (self.position as usize) >= self.buffer.len() {
            return 0.0;
        }

        let sample = if (self.position as usize) < self.buffer.len() - 1 {
            let idx = self.position as usize;
            let frac = self.position - idx as f32;
            self.buffer[idx] * (1.0 - frac) + self.buffer[idx + 1] * frac
        } else {
            self.buffer[self.position as usize]
        };

        self.position += self.pitch;

        if self.loop_enabled && (self.position as usize) >= self.loop_end {
            self.position = self.loop_start as f32 + (self.position - self.loop_end as f32);
        }

        sample
    }
}

impl<const BUF_SIZE: usize> Algorithm<f32> for AkaiS900Emulator<BUF_SIZE> {
    fn process(&mut self, _input: Option<&[f32]>, output: &mut [f32]) -> ProcessResult<()> {
        for out in output.iter_mut() {
            *out = self.generate_sample();
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.position = 0.0;
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Akai S900",
            category: AlgorithmCategory::Generator,
            description: "Akai S900 sampler emulation with interpolation",
            author: "Rill Lo-Fi",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}
