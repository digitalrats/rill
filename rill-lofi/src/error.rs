use thiserror::Error;

/// Errors that can occur during lo-fi audio processing.
#[derive(Error, Debug)]
pub enum LofiError {
    /// Invalid bit depth configuration.
    #[error("Bit depth error: {0}")]
    BitDepth(String),
    /// Invalid sample rate configuration.
    #[error("Sample rate error: {0}")]
    SampleRate(String),
    /// Noise generation error.
    #[error("Noise error: {0}")]
    Noise(String),
    /// General processing error.
    #[error("Processing error: {0}")]
    Processing(String),
}

/// Convenience alias for `Result<T, LofiError>`.
pub type LofiResult<T> = Result<T, LofiError>;
