/// Metadata container for a loaded sample.
pub struct SampleBuffer<T> {
    /// Audio data (mono: single channel interleaved; stereo: deinterleaved into two vecs).
    pub data: Vec<T>,
    /// Right-channel data (None for mono).
    pub right: Option<Vec<T>>,
    /// Original sample rate.
    pub sample_rate: f32,
    /// Number of channels (1 or 2).
    pub channels: u16,
    /// Display name.
    pub name: String,
}

impl<T> SampleBuffer<T> {
    /// Create a mono sample buffer.
    pub fn mono(data: Vec<T>, sample_rate: f32, name: impl Into<String>) -> Self {
        let channels = 1;
        Self {
            data,
            right: None,
            sample_rate,
            channels,
            name: name.into(),
        }
    }

    /// Create a stereo sample buffer (deinterleaved).
    pub fn stereo(left: Vec<T>, right: Vec<T>, sample_rate: f32, name: impl Into<String>) -> Self {
        let channels = 2;
        Self {
            data: left,
            right: Some(right),
            sample_rate,
            channels,
            name: name.into(),
        }
    }

    /// Length in samples (per channel).
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if the sample buffer contains no samples.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
