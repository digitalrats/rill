use thiserror::Error;
use kama_core::traits::AudioError;

#[derive(Error, Debug)]
pub enum LofiError {
    #[error("Bit depth error: {0}")]
    BitDepth(String),
    
    #[error("Sample rate error: {0}")]
    SampleRate(String),
    
    #[error("Noise error: {0}")]
    Noise(String),
    
    #[error("Audio error: {0}")]
    Audio(#[from] AudioError),
}

pub type LofiResult<T> = Result<T, LofiError>;