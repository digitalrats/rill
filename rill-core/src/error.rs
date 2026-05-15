//! # Rill Core error system
//!
//! Centralised error handling for the entire Rill ecosystem.
//! Provides a hierarchy of error types with context and cross-level
//! conversion.

use std::error::Error as StdError;
use std::fmt;

// =============================================================================
// Main error types
// =============================================================================

/// Primary error type for the entire Rill ecosystem.
#[derive(Debug, Clone)]
pub struct Error {
    /// High-level error category for grouping.
    pub category: ErrorCategory,
    /// Machine-processable error code.
    pub code: ErrorCode,
    /// Human-readable error description.
    pub message: String,
    /// Optional chained cause (builder-style via [`Error::with_cause`]).
    pub cause: Option<Box<Error>>,
    /// Optional source location (attached via [`Error::at`]).
    pub location: Option<ErrorLocation>,
}

/// Error category for grouping related error codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Core errors (buffers, queues, basic types).
    Core,
    /// DSP errors (filters, effects, generators).
    Dsp,
    /// Graph errors (connections, topology).
    Graph,
    /// I/O errors (ALSA, JACK, PipeWire).
    Io,
    /// Control errors (MIDI, OSC, automation).
    Control,
    /// Configuration errors.
    Config,
    /// Runtime errors.
    Runtime,
    /// Internal errors (should never occur).
    Internal,
}

impl ErrorCategory {
    /// Return the string representation of this category.
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCategory::Core => "core",
            ErrorCategory::Dsp => "dsp",
            ErrorCategory::Graph => "graph",
            ErrorCategory::Io => "io",
            ErrorCategory::Control => "control",
            ErrorCategory::Config => "config",
            ErrorCategory::Runtime => "runtime",
            ErrorCategory::Internal => "internal",
        }
    }
}

impl fmt::Display for ErrorCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Machine-processable error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // ── Core errors (0-99) ──────────────────────────────────────
    /// Unknown or uncategorised error.
    Unknown = 0,
    /// Invalid parameter value.
    InvalidParameter = 1,
    /// Operation attempted in an invalid state.
    InvalidState = 2,
    /// Unsupported operation.
    Unsupported = 3,
    /// Feature not yet implemented.
    NotImplemented = 4,
    /// Operation timed out.
    Timeout = 5,

    // ── Buffer errors (100-119) ─────────────────────────────────
    /// Buffer is full and cannot accept more data.
    BufferFull = 100,
    /// Buffer is empty and has no data to read.
    BufferEmpty = 101,
    /// Requested buffer size is invalid.
    InvalidBufferSize = 102,
    /// Buffer is misaligned for SIMD operations.
    BufferMisaligned = 103,
    /// Buffer has not been initialised yet.
    BufferNotInitialized = 104,

    // ── Queue errors (120-139) ──────────────────────────────────
    /// Command or telemetry queue is full.
    QueueFull = 120,
    /// Queue is empty (no pending items).
    QueueEmpty = 121,
    /// Queue has been closed.
    QueueClosed = 122,
    /// Queue index is out of bounds.
    InvalidQueueIndex = 123,

    // ── Graph errors (200-299) ──────────────────────────────────
    /// Referenced node does not exist in the graph.
    NodeNotFound = 200,
    /// Referenced port does not exist on the node.
    PortNotFound = 201,
    /// The requested connection is invalid.
    InvalidConnection = 202,
    /// A cycle was detected in the graph (forbidden in a DAG).
    CycleDetected = 203,
    /// Node with the same ID already exists.
    NodeAlreadyExists = 204,
    /// Port is already connected.
    PortAlreadyConnected = 205,

    // ── I/O errors (300-399) ────────────────────────────────────
    /// I/O device not found.
    DeviceNotFound = 300,
    /// I/O device is busy.
    DeviceBusy = 301,
    /// ALSA-specific error.
    AlsaError = 310,
    /// JACK-specific error.
    JackError = 311,
    /// PipeWire-specific error.
    PipeWireError = 312,
    /// Buffer underrun or overrun.
    XRun = 320,

    // ── Control errors (400-499) ────────────────────────────────
    /// MIDI protocol error.
    MidiError = 400,
    /// OSC protocol error.
    OscError = 401,
    /// Control mapping not found.
    MappingNotFound = 402,
    /// Automaton instance not found.
    AutomatonNotFound = 403,
    /// Parameter value is outside the allowed range.
    InvalidParameterValue = 404,

    // ── Config errors (500-599) ─────────────────────────────────
    /// Configuration path not found.
    ConfigNotFound = 500,
    /// Configuration format is invalid.
    InvalidConfigFormat = 501,
    /// Required field is missing from configuration.
    MissingField = 502,

    // ── Runtime errors (600-699) ────────────────────────────────
    /// Real-time safety violation detected.
    RealtimeViolation = 600,
    /// Failed to set thread priority for RT scheduling.
    PriorityError = 601,
    /// Operation failed because the component is already running.
    AlreadyRunning = 602,
    /// Operation failed because the component is not running.
    NotRunning = 603,
}

impl ErrorCode {
    /// Return the error category for this code.
    pub fn category(&self) -> ErrorCategory {
        match *self {
            ErrorCode::Unknown
            | ErrorCode::InvalidParameter
            | ErrorCode::InvalidState
            | ErrorCode::Unsupported
            | ErrorCode::NotImplemented
            | ErrorCode::Timeout
            | ErrorCode::BufferFull
            | ErrorCode::BufferEmpty
            | ErrorCode::InvalidBufferSize
            | ErrorCode::BufferMisaligned
            | ErrorCode::BufferNotInitialized
            | ErrorCode::QueueFull
            | ErrorCode::QueueEmpty
            | ErrorCode::QueueClosed
            | ErrorCode::InvalidQueueIndex => ErrorCategory::Core,

            ErrorCode::NodeNotFound
            | ErrorCode::PortNotFound
            | ErrorCode::InvalidConnection
            | ErrorCode::CycleDetected
            | ErrorCode::NodeAlreadyExists
            | ErrorCode::PortAlreadyConnected => ErrorCategory::Graph,

            ErrorCode::DeviceNotFound
            | ErrorCode::DeviceBusy
            | ErrorCode::AlsaError
            | ErrorCode::JackError
            | ErrorCode::PipeWireError
            | ErrorCode::XRun => ErrorCategory::Io,

            ErrorCode::MidiError
            | ErrorCode::OscError
            | ErrorCode::MappingNotFound
            | ErrorCode::AutomatonNotFound
            | ErrorCode::InvalidParameterValue => ErrorCategory::Control,

            ErrorCode::ConfigNotFound
            | ErrorCode::InvalidConfigFormat
            | ErrorCode::MissingField => ErrorCategory::Config,

            ErrorCode::RealtimeViolation
            | ErrorCode::PriorityError
            | ErrorCode::AlreadyRunning
            | ErrorCode::NotRunning => ErrorCategory::Runtime,
        }
    }

    /// Return a human-readable description of this error code.
    pub fn description(&self) -> &'static str {
        match self {
            ErrorCode::Unknown => "Unknown error",
            ErrorCode::InvalidParameter => "Invalid parameter",
            ErrorCode::InvalidState => "Invalid state",
            ErrorCode::Unsupported => "Unsupported operation",
            ErrorCode::NotImplemented => "Not implemented",
            ErrorCode::Timeout => "Operation timed out",

            ErrorCode::BufferFull => "Buffer is full",
            ErrorCode::BufferEmpty => "Buffer is empty",
            ErrorCode::InvalidBufferSize => "Invalid buffer size",
            ErrorCode::BufferMisaligned => "Buffer is misaligned for SIMD operations",
            ErrorCode::BufferNotInitialized => "Buffer not initialized",

            ErrorCode::QueueFull => "Queue is full",
            ErrorCode::QueueEmpty => "Queue is empty",
            ErrorCode::QueueClosed => "Queue is closed",
            ErrorCode::InvalidQueueIndex => "Invalid queue index",

            ErrorCode::NodeNotFound => "Node not found",
            ErrorCode::PortNotFound => "Port not found",
            ErrorCode::InvalidConnection => "Invalid connection",
            ErrorCode::CycleDetected => "Cycle detected in graph",
            ErrorCode::NodeAlreadyExists => "Node already exists",
            ErrorCode::PortAlreadyConnected => "Port already connected",

            ErrorCode::DeviceNotFound => "Device not found",
            ErrorCode::DeviceBusy => "Device is busy",
            ErrorCode::AlsaError => "ALSA error",
            ErrorCode::JackError => "JACK error",
            ErrorCode::PipeWireError => "PipeWire error",
            ErrorCode::XRun => "Buffer underrun/overrun detected",

            ErrorCode::MidiError => "MIDI error",
            ErrorCode::OscError => "OSC error",
            ErrorCode::MappingNotFound => "Mapping not found",
            ErrorCode::AutomatonNotFound => "Automaton not found",
            ErrorCode::InvalidParameterValue => "Invalid parameter value",

            ErrorCode::ConfigNotFound => "Configuration not found",
            ErrorCode::InvalidConfigFormat => "Invalid configuration format",
            ErrorCode::MissingField => "Missing required field",

            ErrorCode::RealtimeViolation => "Real-time violation detected",
            ErrorCode::PriorityError => "Failed to set thread priority",
            ErrorCode::AlreadyRunning => "Already running",
            ErrorCode::NotRunning => "Not running",
        }
    }
}

/// Source location where an error originated.
#[derive(Debug, Clone)]
pub struct ErrorLocation {
    /// Source file name.
    pub file: &'static str,
    /// Line number in the source file.
    pub line: u32,
    /// Column number in the source file.
    pub column: u32,
}

impl fmt::Display for ErrorLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

// =============================================================================
// Error implementation
// =============================================================================

impl Error {
    /// Create a new error with the given code and message.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            category: code.category(),
            code,
            message: message.into(),
            cause: None,
            location: None,
        }
    }

    /// Add a cause to this error (builder-style).
    pub fn with_cause(mut self, cause: Error) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }

    /// Attach source location info (builder-style).
    pub fn at(mut self, file: &'static str, line: u32, column: u32) -> Self {
        self.location = Some(ErrorLocation { file, line, column });
        self
    }

    /// Walk the cause chain to find the root cause.
    pub fn root_cause(&self) -> &Error {
        let mut current = self;
        while let Some(cause) = &current.cause {
            current = cause;
        }
        current
    }

    /// Whether this error is critical for a real-time thread.
    pub fn is_realtime_critical(&self) -> bool {
        matches!(
            self.code,
            ErrorCode::RealtimeViolation
                | ErrorCode::PriorityError
                | ErrorCode::BufferFull
                | ErrorCode::XRun
        )
    }

    /// Whether this error is recoverable (non-critical).
    pub fn is_recoverable(&self) -> bool {
        !self.is_realtime_critical()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(loc) = &self.location {
            write!(
                f,
                "[{}] at {}: {} ({})",
                self.category,
                loc,
                self.message,
                self.code.description()
            )?;
        } else {
            write!(
                f,
                "[{}]: {} ({})",
                self.category,
                self.message,
                self.code.description()
            )?;
        }

        if let Some(cause) = &self.cause {
            write!(f, "\n  caused by: {}", cause)?;
        }

        Ok(())
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.cause.as_ref().map(|c| c as &dyn StdError)
    }
}

// =============================================================================
// Result type
// =============================================================================

/// Result type alias for Rill Core operations.
pub type Result<T> = std::result::Result<T, Error>;

// =============================================================================
// Conversion from standard errors
// =============================================================================

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::new(ErrorCode::Unknown, err.to_string())
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(err: std::num::ParseIntError) -> Self {
        Error::new(ErrorCode::InvalidParameter, err.to_string())
    }
}

impl From<std::num::ParseFloatError> for Error {
    fn from(err: std::num::ParseFloatError) -> Self {
        Error::new(ErrorCode::InvalidParameter, err.to_string())
    }
}

impl From<std::str::Utf8Error> for Error {
    fn from(err: std::str::Utf8Error) -> Self {
        Error::new(ErrorCode::InvalidParameter, err.to_string())
    }
}

// =============================================================================
// Macros for convenient error creation
// =============================================================================

/// Create an error with a code and message.
#[macro_export]
macro_rules! error {
    ($code:expr, $msg:expr) => {
        $crate::error::Error::new($code, $msg)
    };
    ($code:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::new($code, format!($fmt, $($arg)*))
    };
}

/// Create an error with source location attached.
#[macro_export]
macro_rules! error_at {
    ($code:expr, $msg:expr) => {
        $crate::error::Error::new($code, $msg).at(file!(), line!(), column!())
    };
    ($code:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::new($code, format!($fmt, $($arg)*))
            .at(file!(), line!(), column!())
    };
}

/// Return early with an error (convenience for `return Err(...)`).
#[macro_export]
macro_rules! bail {
    ($code:expr, $msg:expr) => {
        return Err($crate::error::Error::new($code, $msg))
    };
    ($code:expr, $fmt:expr, $($arg:tt)*) => {
        return Err($crate::error::Error::new($code, format!($fmt, $($arg)*)))
    };
}

/// Transform a `Result` by mapping the error with additional context.
#[macro_export]
macro_rules! context {
    ($expr:expr, $code:expr, $msg:expr) => {
        $expr.map_err(|e| $crate::error::Error::new($code, $msg).with_cause(e))
    };
    ($expr:expr, $code:expr, $fmt:expr, $($arg:tt)*) => {
        $expr.map_err(|e| $crate::error::Error::new($code, format!($fmt, $($arg)*)).with_cause(e))
    };
}

// =============================================================================
// Specialized error types for different components
// =============================================================================

/// I/O error constructors.
pub mod io {
    #![allow(unused)]
    use super::*;

    /// Create a `DeviceNotFound` error.
    pub fn device_not_found(name: &str) -> Error {
        error!(ErrorCode::DeviceNotFound, "Device not found: {}", name)
    }

    /// Create a `DeviceBusy` error.
    pub fn device_busy(name: &str) -> Error {
        error!(ErrorCode::DeviceBusy, "Device is busy: {}", name)
    }

    /// Create an `AlsaError` with a description.
    pub fn alsa_error(desc: &str) -> Error {
        error!(ErrorCode::AlsaError, "ALSA error: {}", desc)
    }

    /// Create a `JackError` with a description.
    pub fn jack_error(desc: &str) -> Error {
        error!(ErrorCode::JackError, "JACK error: {}", desc)
    }

    /// Create a `PipeWireError` with a description.
    pub fn pipewire_error(desc: &str) -> Error {
        error!(ErrorCode::PipeWireError, "PipeWire error: {}", desc)
    }

    /// Create an `XRun` (buffer underrun/overrun) error.
    pub fn xrun() -> Error {
        Error::new(ErrorCode::XRun, "Buffer underrun/overrun detected")
    }
}

/// Control error constructors (MIDI, OSC, automation).
pub mod control {
    use super::*;

    /// Create a `MidiError` with a description.
    pub fn midi_error(desc: &str) -> Error {
        error!(ErrorCode::MidiError, "MIDI error: {}", desc)
    }

    /// Create an `OscError` with a description.
    pub fn osc_error(desc: &str) -> Error {
        error!(ErrorCode::OscError, "OSC error: {}", desc)
    }

    /// Create a `MappingNotFound` error for the given mapping ID.
    pub fn mapping_not_found(id: &str) -> Error {
        error!(ErrorCode::MappingNotFound, "Mapping not found: {}", id)
    }

    /// Create an `AutomatonNotFound` error for the given automaton ID.
    pub fn automaton_not_found(id: &str) -> Error {
        error!(ErrorCode::AutomatonNotFound, "Automaton not found: {}", id)
    }

    /// Create an `InvalidParameterValue` error for a value outside the allowed range.
    pub fn invalid_parameter_value(param: &str, value: f64, min: f64, max: f64) -> Error {
        error!(
            ErrorCode::InvalidParameterValue,
            "Invalid value for parameter {}: {} (allowed range: {} - {})", param, value, min, max
        )
    }
}

/// Configuration error constructors.
pub mod config {
    use super::*;

    /// Create a `ConfigNotFound` error for the given path.
    pub fn not_found(path: &str) -> Error {
        error!(
            ErrorCode::ConfigNotFound,
            "Configuration not found: {}", path
        )
    }

    /// Create an `InvalidConfigFormat` error with details.
    pub fn invalid_format(details: &str) -> Error {
        error!(
            ErrorCode::InvalidConfigFormat,
            "Invalid configuration format: {}", details
        )
    }

    /// Create a `MissingField` error for the required field name.
    pub fn missing_field(field: &str) -> Error {
        error!(ErrorCode::MissingField, "Missing required field: {}", field)
    }
}

/// Runtime error constructors (thread priority, critical violations).
pub mod runtime {
    use super::*;

    /// Create a `RealtimeViolation` error with details.
    pub fn realtime_violation(details: &str) -> Error {
        error!(
            ErrorCode::RealtimeViolation,
            "Real-time violation: {}", details
        )
    }

    /// Create a `PriorityError` with details about the failure.
    pub fn priority_error(details: &str) -> Error {
        error!(
            ErrorCode::PriorityError,
            "Failed to set thread priority: {}", details
        )
    }

    /// Create an `AlreadyRunning` error.
    pub fn already_running() -> Error {
        Error::new(ErrorCode::AlreadyRunning, "Already running")
    }

    /// Create a `NotRunning` error.
    pub fn not_running() -> Error {
        Error::new(ErrorCode::NotRunning, "Not running")
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = Error::new(ErrorCode::BufferFull, "Test error");
        assert_eq!(err.code, ErrorCode::BufferFull);
        assert_eq!(err.message, "Test error");
        assert_eq!(err.category, ErrorCategory::Core);
    }

    #[test]
    fn test_error_with_cause() {
        let cause = Error::new(ErrorCode::BufferEmpty, "Cause");
        let err = Error::new(ErrorCode::BufferFull, "Main error").with_cause(cause);

        assert!(err.cause.is_some());
        assert_eq!(err.root_cause().code, ErrorCode::BufferEmpty);
    }

    #[test]
    fn test_error_macros() {
        let err = error!(ErrorCode::BufferFull, "Buffer is full");
        assert_eq!(err.code, ErrorCode::BufferFull);

        let err = error!(ErrorCode::BufferFull, "Buffer {} is full", "test");
        assert_eq!(err.message, "Buffer test is full");
    }

    #[test]
    fn test_specialized_errors() {
        let err = io::device_not_found("hw:0");
        assert_eq!(err.code, ErrorCode::DeviceNotFound);
        assert!(err.message.contains("hw:0"));
    }

    #[test]
    fn test_error_category() {
        assert_eq!(ErrorCode::BufferFull.category(), ErrorCategory::Core);
        assert_eq!(ErrorCode::NodeNotFound.category(), ErrorCategory::Graph);
        assert_eq!(ErrorCode::AlsaError.category(), ErrorCategory::Io);
        assert_eq!(ErrorCode::MidiError.category(), ErrorCategory::Control);
        assert_eq!(ErrorCode::ConfigNotFound.category(), ErrorCategory::Config);
        assert_eq!(
            ErrorCode::RealtimeViolation.category(),
            ErrorCategory::Runtime
        );
    }

    #[test]
    fn test_realtime_critical() {
        assert!(io::xrun().is_realtime_critical());
        assert!(runtime::realtime_violation("test").is_realtime_critical());

        assert!(!config::not_found("test").is_realtime_critical());
    }
}
