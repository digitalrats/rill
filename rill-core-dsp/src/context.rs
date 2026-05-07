//! DSP algorithm execution context

use rill_core::Transcendental;

/// DSP processing context
///
/// Provides information about the current processing state:
/// - timestamps
/// - sample rate
/// - block size
/// - etc.
#[derive(Debug, Clone)]
pub struct DspContext<T: Transcendental> {
    /// Current sample rate
    pub sample_rate: f32,

    /// Current block size
    pub block_size: usize,

    /// Absolute position of the current block (in samples)
    pub block_position: usize,

    /// Data type for current processing
    pub _phantom: std::marker::PhantomData<T>,
}

impl<T: Transcendental> DspContext<T> {
    /// Create a new context
    pub fn new(sample_rate: f32, block_size: usize, block_position: usize) -> Self {
        Self {
            sample_rate,
            block_size,
            block_position,
            _phantom: std::marker::PhantomData,
        }
    }

    /// Get current position in seconds
    pub fn seconds(&self) -> f64 {
        self.block_position as f64 / self.sample_rate as f64
    }
}
