//! # Error Types for Rill Traits
//!
//! This module defines the error types used throughout the Rill ecosystem.
//! All errors implement `std::error::Error` and are designed to be:
//! - Thread-safe (`Send + Sync`)
//! - Cloneable for passing between threads
//! - Human-readable with detailed context
//! - Real-time safe (no allocations in error paths)

use thiserror::Error;

// ============================================================================
// Core Process Error
// ============================================================================

/// Main error type for signal processing operations
///
/// This error can occur during node processing, parameter changes,
/// or any other operation in the signal graph.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum ProcessError {
    /// Error during signal processing
    #[error("Processing error: {0}")]
    Processing(String),

    /// Error with a parameter (invalid value, out of range, etc.)
    #[error("Parameter error: {0}")]
    Parameter(String),

    /// Buffer operation failed
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Type mismatch (e.g., trying to connect signal to control)
    #[error("Type mismatch: expected {expected}, got {got}")]
    TypeMismatch {
        /// Expected type
        expected: &'static str,
        /// Actual type
        got: &'static str,
    },

    /// Sample rate mismatch
    #[error("Sample rate mismatch: expected {expected}, got {got}")]
    SampleRateMismatch {
        /// Expected sample rate
        expected: f32,
        /// Actual sample rate
        got: f32,
    },

    /// Configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// Not initialized
    #[error("Not initialized")]
    NotInitialized,

    /// Already initialized
    #[error("Already initialized")]
    AlreadyInitialized,

    /// Unsupported operation
    #[error("Unsupported operation: {0}")]
    Unsupported(String),

    /// Timeout occurred
    #[error("Operation timed out")]
    Timeout,

    /// Real-time violation — operation exceeded its time budget or
    /// performed an illegal action (allocation, blocking, etc.)
    #[error("Realtime violation: {0}")]
    RealtimeViolation(String),

    /// Internal error (for implementation-specific errors)
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type for signal processing operations
pub type ProcessResult<T> = Result<T, ProcessError>;

impl ProcessError {
    /// Create a new processing error with a formatted message
    pub fn processing(msg: impl Into<String>) -> Self {
        Self::Processing(msg.into())
    }

    /// Create a new parameter error with a formatted message
    pub fn parameter(msg: impl Into<String>) -> Self {
        Self::Parameter(msg.into())
    }

    /// Create a new buffer error
    pub fn buffer(msg: impl Into<String>) -> Self {
        Self::Buffer(msg.into())
    }

    /// Create a new type mismatch error
    pub fn type_mismatch(expected: &'static str, got: &'static str) -> Self {
        Self::TypeMismatch { expected, got }
    }

    /// Create a new sample rate mismatch error
    pub fn sample_rate_mismatch(expected: f32, got: f32) -> Self {
        Self::SampleRateMismatch { expected, got }
    }

    /// Create a new configuration error
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config(msg.into())
    }

    /// Create a new unsupported operation error
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::Unsupported(msg.into())
    }

    /// Create a new internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Check if this error is recoverable
    ///
    /// Recoverable errors are those that don't require stopping the signal thread,
    /// such as temporary buffer underflows or parameter errors.
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::Processing(_) => true,
            Self::Parameter(_) => true,
            Self::Buffer(_) => true,
            Self::TypeMismatch { .. } => false,
            Self::SampleRateMismatch { .. } => false,
            Self::Config(_) => false,
            Self::NotInitialized => true,
            Self::AlreadyInitialized => true,
            Self::Unsupported(_) => false,
            Self::Timeout => true,
            Self::RealtimeViolation(_) => false,
            Self::Internal(_) => false,
        }
    }

    /// Get a short error code for this error (useful for logging)
    pub fn code(&self) -> &'static str {
        match self {
            Self::Processing(_) => "ERR_PROCESSING",
            Self::Parameter(_) => "ERR_PARAMETER",
            Self::Buffer(_) => "ERR_BUFFER",
            Self::TypeMismatch { .. } => "ERR_TYPE_MISMATCH",
            Self::SampleRateMismatch { .. } => "ERR_SAMPLE_RATE",
            Self::Config(_) => "ERR_CONFIG",
            Self::NotInitialized => "ERR_NOT_INIT",
            Self::AlreadyInitialized => "ERR_ALREADY_INIT",
            Self::Unsupported(_) => "ERR_UNSUPPORTED",
            Self::Timeout => "ERR_TIMEOUT",
            Self::RealtimeViolation(_) => "ERR_RT_VIOLATION",
            Self::Internal(_) => "ERR_INTERNAL",
        }
    }
}

// ============================================================================
// Parameter Error
// ============================================================================

/// Errors that can occur during parameter operations
#[derive(Error, Debug, Clone, PartialEq)]
pub enum ParameterError {
    /// Parameter name is empty
    #[error("Parameter name cannot be empty")]
    Empty,

    /// Parameter name contains invalid character
    #[error("Parameter name cannot contain '{0}'")]
    InvalidCharacter(char),

    /// Parameter name is too long
    #[error("Parameter name too long (max {max} characters)")]
    TooLong {
        /// Maximum allowed length
        max: usize,
    },

    /// Parameter name must start with a letter
    #[error("Parameter name must start with a letter")]
    MustStartWithLetter,

    /// Parameter not found
    #[error("Parameter '{0}' not found")]
    NotFound(String),

    /// Parameter type mismatch
    #[error("Parameter type mismatch: expected {expected:?}, got {got:?}")]
    TypeMismatch {
        /// Expected parameter type
        expected: crate::traits::ParamType,
        /// Actual parameter type
        got: crate::traits::ParamType,
    },

    /// Value out of range
    #[error("Value {value} out of range [{min}, {max}]")]
    OutOfRange {
        /// The value that was out of range
        value: f32,
        /// Minimum allowed value
        min: f32,
        /// Maximum allowed value
        max: f32,
    },

    /// Invalid choice (for Choice parameters)
    #[error("Invalid choice '{0}'")]
    InvalidChoice(String),

    /// Duplicate parameter
    #[error("Parameter '{0}' already exists")]
    Duplicate(String),

    /// Parameter is read-only
    #[error("Parameter '{0}' is read-only")]
    ReadOnly(String),
}

/// Result type for parameter operations
pub type ParameterResult<T> = Result<T, ParameterError>;

impl ParameterError {
    /// Create a new not found error
    pub fn not_found(name: impl Into<String>) -> Self {
        Self::NotFound(name.into())
    }

    /// Create a new type mismatch error
    pub fn type_mismatch(
        expected: crate::traits::ParamType,
        got: crate::traits::ParamType,
    ) -> Self {
        Self::TypeMismatch { expected, got }
    }

    /// Create a new out of range error
    pub fn out_of_range(value: f32, min: f32, max: f32) -> Self {
        Self::OutOfRange { value, min, max }
    }

    /// Create a new invalid choice error
    pub fn invalid_choice(choice: impl Into<String>) -> Self {
        Self::InvalidChoice(choice.into())
    }

    /// Create a new duplicate parameter error
    pub fn duplicate(name: impl Into<String>) -> Self {
        Self::Duplicate(name.into())
    }

    /// Create a new read-only error
    pub fn read_only(name: impl Into<String>) -> Self {
        Self::ReadOnly(name.into())
    }
}

// ============================================================================
// Clock Error
// ============================================================================

/// Errors that can occur during clock operations
#[derive(Error, Debug, Clone, PartialEq)]
pub enum ClockError {
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

    /// Clock underflow
    #[error("Clock underflow")]
    Underflow,

    /// Clock overflow
    #[error("Clock overflow")]
    Overflow,
}

/// Result type for clock operations
pub type ClockResult<T> = Result<T, ClockError>;

// ============================================================================
// Conversion Implementations
// ============================================================================

impl From<ParameterError> for ProcessError {
    fn from(err: ParameterError) -> Self {
        match err {
            ParameterError::NotFound(name) => {
                Self::parameter(format!("Parameter not found: {}", name))
            }
            ParameterError::TypeMismatch { expected, got } => {
                Self::type_mismatch(expected.name(), got.name())
            }
            ParameterError::OutOfRange { value, min, max } => {
                Self::parameter(format!("Value {} out of range [{}, {}]", value, min, max))
            }
            ParameterError::InvalidChoice(choice) => {
                Self::parameter(format!("Invalid choice: {}", choice))
            }
            ParameterError::Duplicate(name) => {
                Self::parameter(format!("Duplicate parameter: {}", name))
            }
            ParameterError::ReadOnly(name) => {
                Self::parameter(format!("Parameter is read-only: {}", name))
            }
            _ => Self::parameter(err.to_string()),
        }
    }
}

impl From<ClockError> for ProcessError {
    fn from(err: ClockError) -> Self {
        match err {
            ClockError::Hardware(msg) => Self::processing(format!("Hardware error: {}", msg)),
            ClockError::InvalidSampleRate(sr) => {
                Self::config(format!("Invalid sample rate: {}", sr))
            }
            ClockError::NotStarted => Self::processing("Clock not started"),
            ClockError::AlreadyStarted => Self::processing("Clock already started"),
            ClockError::Underflow => Self::buffer("Clock underflow"),
            ClockError::Overflow => Self::buffer("Clock overflow"),
        }
    }
}

impl From<std::io::Error> for ProcessError {
    fn from(err: std::io::Error) -> Self {
        Self::Processing(format!("IO error: {}", err))
    }
}

impl From<crate::error::Error> for ProcessError {
    fn from(err: crate::error::Error) -> Self {
        Self::Processing(err.to_string())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_error_creation() {
        let err = ProcessError::processing("test error");
        assert!(matches!(err, ProcessError::Processing(_)));
        assert_eq!(err.code(), "ERR_PROCESSING");
        assert!(err.is_recoverable());
    }

    #[test]
    fn test_parameter_error_creation() {
        let err = ParameterError::not_found("gain");
        assert!(matches!(err, ParameterError::NotFound(_)));

        let err = ParameterError::out_of_range(2.0, 0.0, 1.0);
        assert!(matches!(err, ParameterError::OutOfRange { value: 2.0, .. }));
    }

    #[test]
    fn test_error_conversions() {
        let param_err = ParameterError::not_found("test");
        let proc_err: ProcessError = param_err.into();
        assert!(matches!(proc_err, ProcessError::Parameter(_)));

        let clock_err = ClockError::Underflow;
        let proc_err: ProcessError = clock_err.into();
        assert!(matches!(proc_err, ProcessError::Buffer(_)));
    }

    #[test]
    fn test_recoverable_flags() {
        assert!(ProcessError::processing("test").is_recoverable());
        assert!(ProcessError::parameter("test").is_recoverable());
        assert!(ProcessError::buffer("test").is_recoverable());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(ProcessError::processing("").code(), "ERR_PROCESSING");
    }

    #[test]
    fn test_parameter_error_details() {
        let err = ParameterError::out_of_range(1.5, 0.0, 1.0);
        match err {
            ParameterError::OutOfRange { value, min, max } => {
                assert_eq!(value, 1.5);
                assert_eq!(min, 0.0);
                assert_eq!(max, 1.0);
            }
            _ => panic!("Wrong error type"),
        }
    }
}
