//! # Time-related errors
//!
//! This module defines errors that can occur during time and clock operations.

use thiserror::Error;

/// Errors that can occur during time and clock operations
#[derive(Error, Debug, Clone, PartialEq)]
pub enum TimeError {
    /// Hardware error (ALSA, JACK, etc.)
    #[error("Hardware error: {0}")]
    Hardware(String),

    /// Invalid sample rate
    #[error("Invalid sample rate: {0}")]
    InvalidSampleRate(f32),

    /// Clock not started
    #[error("Clock not started")]
    NotStarted,

    /// Clock already started
    #[error("Clock already started")]
    AlreadyStarted,

    /// Clock underflow (data not available in time)
    #[error("Clock underflow")]
    Underflow,

    /// Clock overflow (data produced too fast)
    #[error("Clock overflow")]
    Overflow,

    /// Invalid tempo
    #[error("Invalid tempo: {0}")]
    InvalidTempo(f32),

    /// Timing error
    #[error("Timing error: {0}")]
    Timing(String),
}

impl TimeError {
    /// Create a new hardware error
    pub fn hardware(msg: impl Into<String>) -> Self {
        Self::Hardware(msg.into())
    }

    /// Create a new invalid sample rate error
    pub fn invalid_sample_rate(rate: f32) -> Self {
        Self::InvalidSampleRate(rate)
    }

    /// Create a new invalid tempo error
    pub fn invalid_tempo(tempo: f32) -> Self {
        Self::InvalidTempo(tempo)
    }

    /// Check if the error is recoverable
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::Underflow | Self::Overflow => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_error_creation() {
        let err = TimeError::hardware("ALSA error");
        assert!(matches!(err, TimeError::Hardware(_)));

        let err = TimeError::invalid_sample_rate(96000.0);
        assert!(matches!(err, TimeError::InvalidSampleRate(_)));
    }

    #[test]
    fn test_time_error_recoverable() {
        assert!(TimeError::Underflow.is_recoverable());
        assert!(TimeError::Overflow.is_recoverable());
        assert!(!TimeError::NotStarted.is_recoverable());
    }
}
