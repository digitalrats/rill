//! WAV file loading (feature-gated behind `"wav"`).
//!
//! Supports mono and stereo 16-bit and 24-bit PCM WAV files.

use rill_core::traits::SignalSlab;

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

/// Load a WAV file into a [`SignalSlab`] ready for sampler hot-swap.
///
/// Performs all file I/O and allocations on the calling thread.
/// The resulting `SignalSlab` can be sent to a sampler node via
/// `ParamValue::SignalSlab` and consumed with zero allocations on
/// the real-time I/O thread.
pub fn load_slab(path: &str) -> Result<SignalSlab, WavError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();

    let channels = spec.channels;
    let sample_rate = spec.sample_rate as f32;
    let bits = spec.bits_per_sample;

    let num_frames = reader.duration() as usize;

    let f32_samples: Vec<f32> = match bits {
        16 => reader
            .samples::<i16>()
            .map(|r| r.map(|s| s as f32 / 32768.0))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WavError::Format(format!("Sample read error: {}", e)))?,
        24 => {
            const SCALE: f32 = 1.0 / 8388608.0;
            reader
                .samples::<i32>()
                .map(|r| r.map(|s| s as f32 * SCALE))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| WavError::Format(format!("Sample read error: {}", e)))?
        }
        other => {
            return Err(WavError::Format(format!(
                "Only 16/24-bit supported, got {}-bit",
                other
            )))
        }
    };

    let mut slab_channels: Vec<Box<[f32]>> = Vec::with_capacity(channels as usize);
    if channels == 1 {
        slab_channels.push(f32_samples.into_boxed_slice());
    } else {
        let ch = channels as usize;
        let mut per_channel: Vec<Vec<f32>> =
            (0..ch).map(|_| Vec::with_capacity(num_frames)).collect();
        for chunk in f32_samples.chunks(ch) {
            for (i, &s) in chunk.iter().enumerate() {
                per_channel[i].push(s);
            }
        }
        for v in per_channel {
            slab_channels.push(v.into_boxed_slice());
        }
    }

    Ok(SignalSlab {
        channels: slab_channels,
        sample_rate,
        num_frames,
    })
}
