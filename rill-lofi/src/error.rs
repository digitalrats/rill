use thiserror::Error;

#[derive(Error, Debug)]
pub enum LofiError {
    #[error("Bit depth error: {0}")]
    BitDepth(String),

    #[error("Sample rate error: {0}")]
    SampleRate(String),

    #[error("Noise error: {0}")]
    Noise(String),

    #[error("Processing error: {0}")]
    Processing(String),
}

pub type LofiResult<T> = Result<T, LofiError>;
