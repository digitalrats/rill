//! # Error Types for Rill Traits
//!
//! This module defines the error types used throughout the Rill ecosystem.
//! All errors implement `std::error::Error` and are designed to be:
//! - Thread-safe (`Send + Sync`)
//! - Cloneable for passing between threads
//! - Human-readable with detailed context
//! - Real-time safe (no allocations in error paths)

use std::fmt;
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

    /// Invalid port access
    #[error("Invalid port: {0}")]
    InvalidPort(String),

    /// Buffer operation failed
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Node not found
    #[error("Node {0} not found")]
    NodeNotFound(u32),

    /// Port not found
    #[error("Port {0} not found")]
    PortNotFound(String),

    /// Connection error
    #[error("Connection error: {0}")]
    Connection(String),

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

    /// Create a new port not found error
    pub fn port_not_found(port: impl fmt::Display) -> Self {
        Self::PortNotFound(format!("{}", port))
    }

    /// Create a new connection error
    pub fn connection(msg: impl Into<String>) -> Self {
        Self::Connection(msg.into())
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
            Self::InvalidPort(_) => false,
            Self::Buffer(_) => true,
            Self::NodeNotFound(_) => false,
            Self::PortNotFound(_) => false,
            Self::Connection(_) => false,
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
            Self::InvalidPort(_) => "ERR_INVALID_PORT",
            Self::Buffer(_) => "ERR_BUFFER",
            Self::NodeNotFound(_) => "ERR_NODE_NOT_FOUND",
            Self::PortNotFound(_) => "ERR_PORT_NOT_FOUND",
            Self::Connection(_) => "ERR_CONNECTION",
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
// Port Error
// ============================================================================

/// Errors that can occur during port operations
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum PortError {
    /// Port not found
    #[error("Port {0} not found")]
    NotFound(String),

    /// Port direction mismatch (e.g., trying to connect output to output)
    #[error("Port direction mismatch: expected {expected}, got {got}")]
    DirectionMismatch {
        /// Expected direction
        expected: crate::traits::PortDirection,
        /// Actual direction
        got: crate::traits::PortDirection,
    },

    /// Port type mismatch (e.g., trying to connect signal to control)
    #[error("Port type mismatch: expected {expected:?}, got {got:?}")]
    TypeMismatch {
        /// Expected port type
        expected: crate::traits::PortType,
        /// Actual port type
        got: crate::traits::PortType,
    },

    /// Port already connected
    #[error("Port {0} is already connected")]
    AlreadyConnected(String),

    /// Maximum connections reached
    #[error("Maximum connections reached for port {0}")]
    MaxConnectionsReached(String),

    /// Invalid port index
    #[error("Invalid port index: {0}")]
    InvalidIndex(usize),
}

/// Result type for port operations
pub type PortResult<T> = Result<T, PortError>;

impl PortError {
    /// Create a new not found error
    pub fn not_found(port: impl fmt::Display) -> Self {
        Self::NotFound(format!("{}", port))
    }

    /// Create a new direction mismatch error
    pub fn direction_mismatch(
        expected: crate::traits::PortDirection,
        got: crate::traits::PortDirection,
    ) -> Self {
        Self::DirectionMismatch { expected, got }
    }

    /// Create a new type mismatch error
    pub fn type_mismatch(expected: crate::traits::PortType, got: crate::traits::PortType) -> Self {
        Self::TypeMismatch { expected, got }
    }

    /// Create a new already connected error
    pub fn already_connected(port: impl fmt::Display) -> Self {
        Self::AlreadyConnected(format!("{}", port))
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
// Connection Error
// ============================================================================

/// Errors that can occur during graph connections
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ConnectionError {
    /// Cannot connect node to itself
    #[error("Cannot connect node to itself")]
    SelfConnection,

    /// Cycle detected in graph
    #[error("Cycle detected in graph")]
    CycleDetected,

    /// Connection would create a cycle
    #[error("Connection would create a cycle")]
    WouldCreateCycle,

    /// Invalid connection
    #[error("Invalid connection: {0}")]
    Invalid(String),
}

/// Result type for connection operations
pub type ConnectionResult<T> = Result<T, ConnectionError>;

// ============================================================================
// Error Context (for adding extra information)
// ============================================================================

/// Additional context for errors
///
/// This can be attached to errors to provide more information
/// about where and why they occurred.
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Source location (file:line)
    pub location: Option<String>,

    /// Timestamp when error occurred
    pub timestamp: std::time::SystemTime,

    /// Node ID (if applicable)
    pub node_id: Option<crate::traits::NodeId>,

    /// Port ID (if applicable)
    pub port_id: Option<String>,

    /// Parameter ID (if applicable)
    pub parameter_id: Option<String>,

    /// Additional key-value pairs
    pub extras: Vec<(String, String)>,
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self {
            location: None,
            timestamp: std::time::SystemTime::now(),
            node_id: None,
            port_id: None,
            parameter_id: None,
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

    /// Add node ID
    pub fn with_node(mut self, node_id: crate::traits::NodeId) -> Self {
        self.node_id = Some(node_id);
        self
    }

    /// Add port ID
    pub fn with_port(mut self, port_id: impl fmt::Display) -> Self {
        self.port_id = Some(format!("{}", port_id));
        self
    }

    /// Add parameter ID
    pub fn with_parameter(mut self, param_id: impl AsRef<str>) -> Self {
        self.parameter_id = Some(param_id.as_ref().to_string());
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

        if let Some(node) = self.node_id {
            msg.push_str(&format!("\n  node: {}", node));
        }

        if let Some(port) = &self.port_id {
            msg.push_str(&format!("\n  port: {}", port));
        }

        if let Some(param) = &self.parameter_id {
            msg.push_str(&format!("\n  parameter: {}", param));
        }

        for (key, value) in &self.extras {
            msg.push_str(&format!("\n  {}: {}", key, value));
        }

        msg
    }
}

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

impl From<PortError> for ProcessError {
    fn from(err: PortError) -> Self {
        match err {
            PortError::NotFound(port) => Self::port_not_found(port),
            PortError::DirectionMismatch { expected, got } => {
                Self::type_mismatch(expected.name(), got.name())
            }
            PortError::TypeMismatch { expected, got } => {
                Self::type_mismatch(expected.name(), got.name())
            }
            PortError::AlreadyConnected(port) => {
                Self::connection(format!("Port already connected: {}", port))
            }
            PortError::MaxConnectionsReached(port) => {
                Self::connection(format!("Max connections reached for port: {}", port))
            }
            PortError::InvalidIndex(idx) => {
                Self::invalid_port(format!("Invalid port index: {}", idx))
            }
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

impl From<ConnectionError> for ProcessError {
    fn from(err: ConnectionError) -> Self {
        match err {
            ConnectionError::SelfConnection => Self::connection("Cannot connect node to itself"),
            ConnectionError::CycleDetected => Self::connection("Cycle detected in graph"),
            ConnectionError::WouldCreateCycle => {
                Self::connection("Connection would create a cycle")
            }
            ConnectionError::Invalid(msg) => Self::connection(msg),
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
    use crate::traits::{PortDirection, PortType};

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
    fn test_parameter_error_creation() {
        let err = ParameterError::not_found("gain");
        assert!(matches!(err, ParameterError::NotFound(_)));

        let err = ParameterError::out_of_range(2.0, 0.0, 1.0);
        assert!(matches!(err, ParameterError::OutOfRange { value: 2.0, .. }));
    }

    #[test]
    fn test_port_error_creation() {
        let err = PortError::direction_mismatch(PortDirection::Input, PortDirection::Output);
        assert!(matches!(err, PortError::DirectionMismatch { .. }));

        let err = PortError::type_mismatch(PortType::Signal, PortType::Control);
        assert!(matches!(err, PortError::TypeMismatch { .. }));
    }

    #[test]
    fn test_error_conversions() {
        let param_err = ParameterError::not_found("test");
        let proc_err: ProcessError = param_err.into();
        assert!(matches!(proc_err, ProcessError::Parameter(_)));

        let port_err = PortError::not_found("port");
        let proc_err: ProcessError = port_err.into();
        assert!(matches!(proc_err, ProcessError::PortNotFound(_)));

        let clock_err = ClockError::Underflow;
        let proc_err: ProcessError = clock_err.into();
        assert!(matches!(proc_err, ProcessError::Buffer(_)));
    }

    #[test]
    fn test_error_context() {
        let ctx = ErrorContext::new()
            .with_location("test.rs", 42)
            .with_node(crate::traits::NodeId(1))
            .with_extra("sample_rate", "44100");

        let err = ProcessError::processing("test");
        let formatted = ctx.format(&err);

        assert!(formatted.contains("test.rs:42"));
        assert!(formatted.contains("node: Node(1)"));
        assert!(formatted.contains("sample_rate: 44100"));
    }

    #[test]
    fn test_recoverable_flags() {
        assert!(ProcessError::processing("test").is_recoverable());
        assert!(ProcessError::parameter("test").is_recoverable());
        assert!(ProcessError::buffer("test").is_recoverable());
        assert!(!ProcessError::node_not_found(42).is_recoverable());
        assert!(!ProcessError::port_not_found("port").is_recoverable());
    }

    #[test]
    fn test_error_codes() {
        assert_eq!(ProcessError::processing("").code(), "ERR_PROCESSING");
        assert_eq!(ProcessError::node_not_found(0).code(), "ERR_NODE_NOT_FOUND");
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
