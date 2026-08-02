//! Recording sink — captures signal into a `Vec<f32>` for offline analysis.

use std::sync::{Arc, Mutex};

/// Recording sink that captures signal samples into a shared buffer.
pub struct RecordingSink<const B: usize> {
    /// Shared buffer where recorded samples are stored.
    pub recorded: Arc<Mutex<Vec<f32>>>,
}

impl<const B: usize> RecordingSink<B> {
    /// Creates a new recording sink writing to the given buffer.
    pub fn new(recorded: Arc<Mutex<Vec<f32>>>) -> Self {
        Self { recorded }
    }

    /// Records a slice of samples into the shared buffer.
    pub fn record(&self, samples: &[f32]) {
        if let Ok(mut buf) = self.recorded.lock() {
            buf.extend_from_slice(samples);
        }
    }

    /// Writes recorded samples to a WAV file.
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
