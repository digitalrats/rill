//! WAV file loading (feature-gated behind `"wav"`).
//!
//! Supports mono and stereo 16-bit and 24-bit PCM WAV files.

use crate::buffer::SampleBuffer;
use rill_core::prelude::Sample;

/// Errors that can occur during WAV loading.
#[derive(Debug)]
pub enum WavError {
    /// An I/O error occurred while reading the file.
    Io(std::io::Error),
    /// The WAV file could not be decoded by the `hound` crate.
    Hound(String),
    /// The WAV format is unsupported (not 16-bit PCM or not mono/stereo).
    Format(String),
}

impl std::fmt::Display for WavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WavError::Io(e) => write!(f, "IO error: {}", e),
            WavError::Hound(s) => write!(f, "WAV decode error: {}", s),
            WavError::Format(s) => write!(f, "Invalid WAV: {}", s),
        }
    }
}

impl std::error::Error for WavError {}

impl From<std::io::Error> for WavError {
    fn from(e: std::io::Error) -> Self {
        WavError::Io(e)
    }
}

impl From<hound::Error> for WavError {
    fn from(e: hound::Error) -> Self {
        WavError::Hound(e.to_string())
    }
}

/// Load a WAV file into a `SampleBuffer<Sample>`.
///
/// Supports 16-bit and 24-bit PCM, mono and stereo.
pub fn load_wav(path: &str) -> Result<SampleBuffer<Sample>, WavError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();

    let channels = spec.channels;
    let sample_rate = spec.sample_rate as f32;
    let bits_per_sample = spec.bits_per_sample;

    if channels != 1 && channels != 2 {
        return Err(WavError::Format(format!(
            "Only mono/stereo supported, got {} channels",
            channels
        )));
    }

    let name = path.rsplit('/').next().unwrap_or(path).to_string();

    match bits_per_sample {
        16 => {
            let samples: Vec<i16> = reader.samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| WavError::Format(format!("Sample read error: {}", e)))?;
            Ok(samples_to_buffer(samples.into_iter().map(|s| s as f32 / 32768.0), channels, sample_rate, name))
        }
        24 => {
            let samples: Vec<i32> = reader.samples::<i32>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| WavError::Format(format!("Sample read error: {}", e)))?;
            const SCALE: f32 = 1.0 / 8388608.0; // 2^23
            Ok(samples_to_buffer(samples.into_iter().map(|s| s as f32 * SCALE), channels, sample_rate, name))
        }
        other => Err(WavError::Format(format!(
            "Only 16/24-bit PCM supported, got {}-bit", other
        ))),
    }
}

fn samples_to_buffer(
    samples: impl Iterator<Item = f32>,
    channels: u16,
    sample_rate: f32,
    name: String,
) -> SampleBuffer<Sample> {
    let data: Vec<Sample> = samples.collect();
    if channels == 1 {
        SampleBuffer::mono(data, sample_rate, name)
    } else {
        let mut left = Vec::with_capacity(data.len() / 2);
        let mut right = Vec::with_capacity(data.len() / 2);
        for chunk in data.chunks(2) {
            left.push(chunk[0]);
            if chunk.len() > 1 {
                right.push(chunk[1]);
            }
        }
        SampleBuffer::stereo(left, right, sample_rate, name)
    }
}
