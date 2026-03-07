//! # Error Types for Kama Core
//!
//! This module defines the error types used throughout the Kama Audio ecosystem.

use thiserror::Error;
use std::fmt;

// ============================================================================
// Core Process Error
// ============================================================================

/// Main error type for audio processing operations
#[derive(Error, Debug, Clone, PartialEq)]  // Убрали Eq
pub enum ProcessError {
    /// Error during audio processing
    #[error("Processing error: {0}")]
    Processing(String),
    
    /// Error with a parameter (invalid value, out of range, etc.)
    #[error("Parameter error: {0}")]
    Parameter(String),
    
    /// Invalid port access
    #[error("Invalid port: {0}")]
    InvalidPort(String),
    
    /// Buffer operation failed
    #[error("Buffer error: {0}")]
    Buffer(String),
    
    /// Node not found
    #[error("Node {0} not found")]
    NodeNotFound(u32),
    
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
    
    /// Internal error (for implementation-specific errors)
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Result type for audio processing operations
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
    
    /// Create a new invalid port error
    pub fn invalid_port(port: impl fmt::Display) -> Self {
        Self::InvalidPort(format!("Invalid port: {}", port))
    }
    
    /// Create a new buffer error
    pub fn buffer(msg: impl Into<String>) -> Self {
        Self::Buffer(msg.into())
    }
    
    /// Create a new node not found error
    pub fn node_not_found(id: u32) -> Self {
        Self::NodeNotFound(id)
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
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::Processing(_) => true,
            Self::Parameter(_) => true,
            Self::InvalidPort(_) => false,
            Self::Buffer(_) => true,
            Self::NodeNotFound(_) => false,
            Self::SampleRateMismatch { .. } => false,
            Self::Config(_) => false,
            Self::NotInitialized => true,
            Self::AlreadyInitialized => true,
            Self::Unsupported(_) => false,
            Self::Timeout => true,
            Self::Internal(_) => false,
        }
    }
    
    /// Get a short error code for this error
    pub fn code(&self) -> &'static str {
        match self {
            Self::Processing(_) => "ERR_PROCESSING",
            Self::Parameter(_) => "ERR_PARAMETER",
            Self::InvalidPort(_) => "ERR_INVALID_PORT",
            Self::Buffer(_) => "ERR_BUFFER",
            Self::NodeNotFound(_) => "ERR_NODE_NOT_FOUND",
            Self::SampleRateMismatch { .. } => "ERR_SAMPLE_RATE",
            Self::Config(_) => "ERR_CONFIG",
            Self::NotInitialized => "ERR_NOT_INIT",
            Self::AlreadyInitialized => "ERR_ALREADY_INIT",
            Self::Unsupported(_) => "ERR_UNSUPPORTED",
            Self::Timeout => "ERR_TIMEOUT",
            Self::Internal(_) => "ERR_INTERNAL",
        }
    }
}

// ============================================================================
// Buffer Error
// ============================================================================

/// Errors that can occur during buffer operations
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]  // BufferError может быть Eq
pub enum BufferError {
    /// Buffer is empty (tried to read when no data available)
    #[error("Buffer is empty")]
    Empty,
    
    /// Buffer is full (tried to write when no space available)
    #[error("Buffer is full")]
    Full,
    
    /// Invalid index access
    #[error("Invalid index: {0}")]
    InvalidIndex(usize),
    
    /// Buffer is disconnected (other end is gone)
    #[error("Buffer is disconnected")]
    Disconnected,
    
    /// Operation would block (for non-blocking operations)
    #[error("Operation would block")]
    WouldBlock,
    
    /// Buffer overflow (data was lost)
    #[error("Buffer overflow")]
    Overflow,
    
    /// Buffer underflow (no data available)
    #[error("Buffer underflow")]
    Underflow,
    
    /// Invalid buffer size
    #[error("Invalid buffer size: {0}")]
    InvalidSize(usize),
    
    /// Misaligned access
    #[error("Misaligned access")]
    Misaligned,
}

/// Result type for buffer operations
pub type BufferResult<T> = Result<T, BufferError>;

impl BufferError {
    /// Check if this error is recoverable
    pub fn is_recoverable(&self) -> bool {
        match self {
            Self::Empty | Self::Full | Self::WouldBlock | Self::Overflow | Self::Underflow => true,
            Self::InvalidIndex(_) | Self::Disconnected | Self::InvalidSize(_) | Self::Misaligned => false,
        }
    }
    
    /// Get a short error code
    pub fn code(&self) -> &'static str {
        match self {
            Self::Empty => "ERR_BUF_EMPTY",
            Self::Full => "ERR_BUF_FULL",
            Self::InvalidIndex(_) => "ERR_BUF_INVALID_INDEX",
            Self::Disconnected => "ERR_BUF_DISCONNECTED",
            Self::WouldBlock => "ERR_BUF_WOULD_BLOCK",
            Self::Overflow => "ERR_BUF_OVERFLOW",
            Self::Underflow => "ERR_BUF_UNDERFLOW",
            Self::InvalidSize(_) => "ERR_BUF_INVALID_SIZE",
            Self::Misaligned => "ERR_BUF_MISALIGNED",
        }
    }
}

// ============================================================================
// Parameter Error
// ============================================================================

/// Errors that can occur during parameter operations
#[derive(Error, Debug, Clone, PartialEq)]  // Убрали Eq
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
        expected: super::traits::ParamType,
        /// Actual parameter type
        got: super::traits::ParamType,
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
}

/// Result type for parameter operations
pub type ParameterResult<T> = Result<T, ParameterError>;

impl ParameterError {
    /// Create a new not found error
    pub fn not_found(name: impl Into<String>) -> Self {
        Self::NotFound(name.into())
    }
    
    /// Create a new type mismatch error
    pub fn type_mismatch(expected: super::traits::ParamType, got: super::traits::ParamType) -> Self {
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
}

// ============================================================================
// Queue Error
// ============================================================================

/// Errors that can occur during queue operations
#[derive(Error, Debug, Clone, PartialEq, Eq)]  // QueueError может быть Eq
pub enum QueueError {
    /// Queue is full
    #[error("Queue is full")]
    QueueFull,
    
    /// Queue is empty
    #[error("Queue is empty")]
    QueueEmpty,
    
    /// Channel is disconnected (all senders/receivers dropped)
    #[error("Channel disconnected")]
    ChannelDisconnected,
    
    /// Operation timed out
    #[error("Operation timed out")]
    Timeout,
    
    /// Operation not supported for this queue type
    #[error("Operation not supported: {0}")]
    Unsupported(String),
    
    /// Send failed with data loss
    #[error("Send failed: {0}")]
    SendFailed(String),
    
    /// Receive failed
    #[error("Receive failed: {0}")]
    ReceiveFailed(String),
    
    /// Invalid queue configuration
    #[error("Invalid queue configuration: {0}")]
    InvalidConfig(String),
}

/// Result type for queue operations
pub type QueueResult<T> = Result<T, QueueError>;

impl QueueError {
    /// Create a new unsupported error
    pub fn unsupported(msg: impl Into<String>) -> Self {
        Self::Unsupported(msg.into())
    }
    
    /// Create a new send failed error
    pub fn send_failed(msg: impl Into<String>) -> Self {
        Self::SendFailed(msg.into())
    }
    
    /// Create a new receive failed error
    pub fn receive_failed(msg: impl Into<String>) -> Self {
        Self::ReceiveFailed(msg.into())
    }
    
    /// Create a new invalid config error
    pub fn invalid_config(msg: impl Into<String>) -> Self {
        Self::InvalidConfig(msg.into())
    }
}

// ============================================================================
// Conversion Implementations
// ============================================================================

impl From<BufferError> for ProcessError {
    fn from(err: BufferError) -> Self {
        Self::Buffer(err.to_string())
    }
}

impl From<ParameterError> for ProcessError {
    fn from(err: ParameterError) -> Self {
        match err {
            ParameterError::NotFound(name) => Self::parameter(format!("Parameter not found: {}", name)),
            ParameterError::TypeMismatch { expected, got } => {
                Self::parameter(format!("Type mismatch: expected {:?}, got {:?}", expected, got))
            }
            ParameterError::OutOfRange { value, min, max } => {
                Self::parameter(format!("Value {} out of range [{}, {}]", value, min, max))
            }
            _ => Self::parameter(err.to_string()),
        }
    }
}

impl From<std::io::Error> for ProcessError {
    fn from(err: std::io::Error) -> Self {
        Self::Processing(format!("IO error: {}", err))
    }
}

impl<T> From<crossbeam_channel::TrySendError<T>> for QueueError {
    fn from(err: crossbeam_channel::TrySendError<T>) -> Self {
        match err {
            crossbeam_channel::TrySendError::Full(_) => QueueError::QueueFull,
            crossbeam_channel::TrySendError::Disconnected(_) => QueueError::ChannelDisconnected,
        }
    }
}

impl From<crossbeam_channel::TryRecvError> for QueueError {
    fn from(err: crossbeam_channel::TryRecvError) -> Self {
        match err {
            crossbeam_channel::TryRecvError::Empty => QueueError::QueueEmpty,
            crossbeam_channel::TryRecvError::Disconnected => QueueError::ChannelDisconnected,
        }
    }
}

// ============================================================================
// Error Context (for adding extra information)
// ============================================================================

/// Additional context for errors
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Source location (file:line)
    pub location: Option<String>,
    
    /// Thread ID where error occurred
    pub thread_id: Option<std::thread::ThreadId>,
    
    /// Timestamp when error occurred
    pub timestamp: std::time::SystemTime,
    
    /// Additional key-value pairs
    pub extras: Vec<(String, String)>,
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self {
            location: None,
            thread_id: Some(std::thread::current().id()),
            timestamp: std::time::SystemTime::now(),
            extras: Vec::new(),
        }
    }
}

impl ErrorContext {
    /// Create new error context
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Add source location
    pub fn with_location(mut self, file: &str, line: u32) -> Self {
        self.location = Some(format!("{}:{}", file, line));
        self
    }
    
    /// Add extra key-value pair
    pub fn with_extra(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extras.push((key.into(), value.into()));
        self
    }
    
    /// Format error with context
    pub fn format(&self, error: &impl std::error::Error) -> String {
        let mut msg = format!("{}", error);
        
        if let Some(loc) = &self.location {
            msg.push_str(&format!("\n  at {}", loc));
        }
        
        if let Some(id) = self.thread_id {
            msg.push_str(&format!("\n  thread: {:?}", id));
        }
        
        for (key, value) in &self.extras {
            msg.push_str(&format!("\n  {}: {}", key, value));
        }
        
        msg
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
        
        let err = ProcessError::node_not_found(42);
        assert!(matches!(err, ProcessError::NodeNotFound(42)));
        assert_eq!(err.code(), "ERR_NODE_NOT_FOUND");
        assert!(!err.is_recoverable());
    }
    
    #[test]
    fn test_buffer_error_creation() {
        let err = BufferError::Empty;
        assert!(matches!(err, BufferError::Empty));
        assert_eq!(err.code(), "ERR_BUF_EMPTY");
        assert!(err.is_recoverable());
        
        let err = BufferError::InvalidIndex(5);
        assert!(matches!(err, BufferError::InvalidIndex(5)));
        assert_eq!(err.code(), "ERR_BUF_INVALID_INDEX");
        assert!(!err.is_recoverable());
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
        let buf_err = BufferError::Empty;
        let proc_err: ProcessError = buf_err.into();
        assert!(matches!(proc_err, ProcessError::Buffer(_)));
    }
}