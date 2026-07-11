//! Recording sink — captures signal into a Vec<f32> for offline analysis.

use std::sync::{Arc, Mutex};

pub struct RecordingSink<const B: usize> {
    pub recorded: Arc<Mutex<Vec<f32>>>,
}

impl<const B: usize> RecordingSink<B> {
    pub fn new(recorded: Arc<Mutex<Vec<f32>>>) -> Self {
        Self { recorded }
    }

    pub fn record(&self, samples: &[f32]) {
        if let Ok(mut buf) = self.recorded.lock() {
            buf.extend_from_slice(samples);
        }
    }

    #[cfg(feature = "wav")]
    pub fn write_wav(
        path: &str,
        sample_rate: u32,
        channels: u16,
        samples: &[f32],
    ) -> Result<(), String> {
        let spec = hound::WavSpec {
            channels,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
        for &s in samples {
            writer
                .write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)
                .map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())
    }
}
