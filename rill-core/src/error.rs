//! # Система ошибок Rill Core
//!
//! Централизованная система обработки ошибок для всей экосистемы Rill.
//! Предоставляет иерархию типов ошибок с контекстом и возможностью
//! преобразования между различными уровнями.

use std::fmt;
use std::error::Error as StdError;

// =============================================================================
// Основные типы ошибок
// =============================================================================

/// Основной тип ошибки для всей экосистемы Rill
#[derive(Debug, Clone)]
pub struct Error {
    /// Категория ошибки
    pub category: ErrorCategory,
    /// Код ошибки (для машинной обработки)
    pub code: ErrorCode,
    /// Человекочитаемое сообщение
    pub message: String,
    /// Причина (опционально)
    pub cause: Option<Box<Error>>,
    /// Место возникновения (файл, строка)
    pub location: Option<ErrorLocation>,
}

/// Категория ошибки
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// Ошибки ядра (буферы, очереди, базовые типы)
    Core,
    /// Ошибки DSP (фильтры, эффекты, генераторы)
    Dsp,
    /// Ошибки графа (соединения, топология)
    Graph,
    /// Ошибки ввода-вывода (ALSA, JACK, PipeWire)
    Io,
    /// Ошибки управления (MIDI, OSC, автоматизация)
    Control,
    /// Ошибки конфигурации
    Config,
    /// Ошибки времени выполнения
    Runtime,
    /// Внутренние ошибки (не должны возникать)
    Internal,
}

impl ErrorCategory {
    /// Получить строковое представление
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

/// Код ошибки (для машинной обработки)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    // =======================================================================
    // Core errors (0-99)
    // =======================================================================
    /// Неизвестная ошибка
    Unknown = 0,
    /// Неверный параметр
    InvalidParameter = 1,
    /// Неверное состояние
    InvalidState = 2,
    /// Неподдерживаемая операция
    Unsupported = 3,
    /// Не реализовано
    NotImplemented = 4,
    /// Таймаут
    Timeout = 5,
    
    // =======================================================================
    // Buffer errors (100-119)
    // =======================================================================
    /// Переполнение буфера
    BufferFull = 100,
    /// Буфер пуст
    BufferEmpty = 101,
    /// Неверный размер буфера
    InvalidBufferSize = 102,
    /// Неверное выравнивание буфера
    BufferMisaligned = 103,
    /// Буфер не инициализирован
    BufferNotInitialized = 104,
    
    // =======================================================================
    // Queue errors (120-139)
    // =======================================================================
    /// Очередь переполнена
    QueueFull = 120,
    /// Очередь пуста
    QueueEmpty = 121,
    /// Очередь закрыта
    QueueClosed = 122,
    /// Неверный индекс очереди
    InvalidQueueIndex = 123,
    
    // =======================================================================
    // Graph errors (200-299)
    // =======================================================================
    /// Узел не найден
    NodeNotFound = 200,
    /// Порт не найден
    PortNotFound = 201,
    /// Неверное соединение
    InvalidConnection = 202,
    /// Цикл в графе
    CycleDetected = 203,
    /// Узел уже существует
    NodeAlreadyExists = 204,
    /// Порт уже подключён
    PortAlreadyConnected = 205,
    
    // =======================================================================
    // IO errors (300-399)
    // =======================================================================
    /// Устройство не найдено
    DeviceNotFound = 300,
    /// Устройство занято
    DeviceBusy = 301,
    /// Ошибка ALSA
    AlsaError = 310,
    /// Ошибка JACK
    JackError = 311,
    /// Ошибка PipeWire
    PipeWireError = 312,
    /// XRun (переполнение/опустошение буфера)
    XRun = 320,
    
    // =======================================================================
    // Control errors (400-499)
    // =======================================================================
    /// MIDI ошибка
    MidiError = 400,
    /// OSC ошибка
    OscError = 401,
    /// Маппинг не найден
    MappingNotFound = 402,
    /// Автомат не найден
    AutomatonNotFound = 403,
    /// Неверное значение параметра
    InvalidParameterValue = 404,
    
    // =======================================================================
    // Config errors (500-599)
    // =======================================================================
    /// Конфигурация не найдена
    ConfigNotFound = 500,
    /// Неверный формат конфигурации
    InvalidConfigFormat = 501,
    /// Отсутствует обязательное поле
    MissingField = 502,
    
    // =======================================================================
    // Runtime errors (600-699)
    // =======================================================================
    /// Ошибка в real-time потоке
    RealtimeViolation = 600,
    /// Приоритет потока не может быть установлен
    PriorityError = 601,
    /// Поток уже запущен
    AlreadyRunning = 602,
    /// Поток не запущен
    NotRunning = 603,
}

impl ErrorCode {
    /// Получить категорию ошибки
    pub fn category(&self) -> ErrorCategory {
        match *self {
            ErrorCode::Unknown | ErrorCode::InvalidParameter | ErrorCode::InvalidState
            | ErrorCode::Unsupported | ErrorCode::NotImplemented | ErrorCode::Timeout
            | ErrorCode::BufferFull | ErrorCode::BufferEmpty | ErrorCode::InvalidBufferSize
            | ErrorCode::BufferMisaligned | ErrorCode::BufferNotInitialized
            | ErrorCode::QueueFull | ErrorCode::QueueEmpty | ErrorCode::QueueClosed
            | ErrorCode::InvalidQueueIndex => ErrorCategory::Core,
            
            ErrorCode::NodeNotFound | ErrorCode::PortNotFound | ErrorCode::InvalidConnection
            | ErrorCode::CycleDetected | ErrorCode::NodeAlreadyExists
            | ErrorCode::PortAlreadyConnected => ErrorCategory::Graph,
            
            ErrorCode::DeviceNotFound | ErrorCode::DeviceBusy | ErrorCode::AlsaError
            | ErrorCode::JackError | ErrorCode::PipeWireError | ErrorCode::XRun => ErrorCategory::Io,
            
            ErrorCode::MidiError | ErrorCode::OscError | ErrorCode::MappingNotFound
            | ErrorCode::AutomatonNotFound | ErrorCode::InvalidParameterValue => ErrorCategory::Control,
            
            ErrorCode::ConfigNotFound | ErrorCode::InvalidConfigFormat
            | ErrorCode::MissingField => ErrorCategory::Config,
            
            ErrorCode::RealtimeViolation | ErrorCode::PriorityError
            | ErrorCode::AlreadyRunning | ErrorCode::NotRunning => ErrorCategory::Runtime,
        }
    }
    
    /// Получить описание ошибки
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

/// Место возникновения ошибки
#[derive(Debug, Clone)]
pub struct ErrorLocation {
    /// Файл
    pub file: &'static str,
    /// Строка
    pub line: u32,
    /// Колонка
    pub column: u32,
}

impl fmt::Display for ErrorLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.file, self.line, self.column)
    }
}

// =============================================================================
// Реализация Error
// =============================================================================

impl Error {
    /// Создать новую ошибку
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            category: code.category(),
            code,
            message: message.into(),
            cause: None,
            location: None,
        }
    }
    
    /// Создать ошибку с причиной
    pub fn with_cause(mut self, cause: Error) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
    
    /// Добавить информацию о месте возникновения
    pub fn at(mut self, file: &'static str, line: u32, column: u32) -> Self {
        self.location = Some(ErrorLocation { file, line, column });
        self
    }
    
    /// Получить корневую причину
    pub fn root_cause(&self) -> &Error {
        let mut current = self;
        while let Some(cause) = &current.cause {
            current = cause;
        }
        current
    }
    
    /// Проверить, является ли ошибка фатальной для RT-потока
    pub fn is_realtime_critical(&self) -> bool {
        matches!(self.code, 
            ErrorCode::RealtimeViolation 
            | ErrorCode::PriorityError
            | ErrorCode::BufferFull
            | ErrorCode::XRun
        )
    }
    
    /// Проверить, является ли ошибка recoverable
    pub fn is_recoverable(&self) -> bool {
        !self.is_realtime_critical()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(loc) = &self.location {
            write!(f, "[{}] at {}: {} ({})", 
                self.category, loc, self.message, self.code.description())?;
        } else {
            write!(f, "[{}]: {} ({})", 
                self.category, self.message, self.code.description())?;
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
// Результат операций
// =============================================================================

/// Результат операций в Rill Core
pub type Result<T> = std::result::Result<T, Error>;

// =============================================================================
// Конвертация из стандартных ошибок
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
// Макросы для удобного создания ошибок
// =============================================================================

/// Создать ошибку с кодом и сообщением
#[macro_export]
macro_rules! error {
    ($code:expr, $msg:expr) => {
        $crate::error::Error::new($code, $msg)
    };
    ($code:expr, $fmt:expr, $($arg:tt)*) => {
        $crate::error::Error::new($code, format!($fmt, $($arg)*))
    };
}

/// Создать ошибку с местом возникновения
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

/// Возврат ошибки с контекстом
#[macro_export]
macro_rules! bail {
    ($code:expr, $msg:expr) => {
        return Err($crate::error::Error::new($code, $msg))
    };
    ($code:expr, $fmt:expr, $($arg:tt)*) => {
        return Err($crate::error::Error::new($code, format!($fmt, $($arg)*)))
    };
}

/// Преобразование Result с добавлением контекста
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
// Специализированные типы ошибок для разных компонентов
// =============================================================================

/// Ошибки буферов
pub mod buffer {
    use super::*;
    #[allow(dead_code)]
    pub fn full() -> Error {
        Error::new(ErrorCode::BufferFull, "Buffer is full")
    }
    #[allow(dead_code)]
    pub fn empty() -> Error {
        Error::new(ErrorCode::BufferEmpty, "Buffer is empty")
    }
    #[allow(dead_code)]
    pub fn invalid_size(expected: usize, got: usize) -> Error {
        error!(
            ErrorCode::InvalidBufferSize,
            "Invalid buffer size: expected {}, got {}", expected, got
        )
    }
    #[allow(dead_code)]
    pub fn misaligned(required: usize, actual: usize) -> Error {
        error!(
            ErrorCode::BufferMisaligned,
            "Buffer misaligned: required {} byte alignment, actual {}",
            required, actual
        )
    }
    #[allow(dead_code)]
    pub fn not_initialized() -> Error {
        Error::new(ErrorCode::BufferNotInitialized, "Buffer not initialized")
    }
}

/// Ошибки очередей
pub mod queue {
    use super::*;
    
    pub fn full() -> Error {
        Error::new(ErrorCode::QueueFull, "Queue is full")
    }
    
    pub fn empty() -> Error {
        Error::new(ErrorCode::QueueEmpty, "Queue is empty")
    }
    
    pub fn closed() -> Error {
        Error::new(ErrorCode::QueueClosed, "Queue is closed")
    }
    
    pub fn invalid_index(idx: usize, max: usize) -> Error {
        error!(
            ErrorCode::InvalidQueueIndex,
            "Invalid queue index: {} (max {})", idx, max
        )
    }
}

/// Ошибки графа
pub mod graph {
    use super::*;
    use crate::traits::NodeId;
    use crate::traits::PortId;
    
    pub fn node_not_found(id: NodeId) -> Error {
        error!(ErrorCode::NodeNotFound, "Node not found: {}", id)
    }
    
    pub fn port_not_found(id: PortId) -> Error {
        error!(ErrorCode::PortNotFound, "Port not found: {}", id)
    }
    
    pub fn invalid_connection(from: PortId, to: PortId) -> Error {
        error!(
            ErrorCode::InvalidConnection,
            "Invalid connection: {} -> {}", from, to
        )
    }
    
    pub fn cycle_detected() -> Error {
        Error::new(ErrorCode::CycleDetected, "Cycle detected in graph")
    }
    
    pub fn node_already_exists(id: NodeId) -> Error {
        error!(ErrorCode::NodeAlreadyExists, "Node already exists: {}", id)
    }
    
    pub fn port_already_connected(port: PortId) -> Error {
        error!(ErrorCode::PortAlreadyConnected, "Port already connected: {}", port)
    }
}

/// Ошибки ввода-вывода
pub mod io {
    use super::*;
    
    pub fn device_not_found(name: &str) -> Error {
        error!(ErrorCode::DeviceNotFound, "Device not found: {}", name)
    }
    
    pub fn device_busy(name: &str) -> Error {
        error!(ErrorCode::DeviceBusy, "Device is busy: {}", name)
    }
    
    pub fn alsa_error(desc: &str) -> Error {
        error!(ErrorCode::AlsaError, "ALSA error: {}", desc)
    }
    
    pub fn jack_error(desc: &str) -> Error {
        error!(ErrorCode::JackError, "JACK error: {}", desc)
    }
    
    pub fn pipewire_error(desc: &str) -> Error {
        error!(ErrorCode::PipeWireError, "PipeWire error: {}", desc)
    }
    
    pub fn xrun() -> Error {
        Error::new(ErrorCode::XRun, "Buffer underrun/overrun detected")
    }
}

/// Ошибки управления
pub mod control {
    use super::*;
    
    pub fn midi_error(desc: &str) -> Error {
        error!(ErrorCode::MidiError, "MIDI error: {}", desc)
    }
    
    pub fn osc_error(desc: &str) -> Error {
        error!(ErrorCode::OscError, "OSC error: {}", desc)
    }
    
    pub fn mapping_not_found(id: &str) -> Error {
        error!(ErrorCode::MappingNotFound, "Mapping not found: {}", id)
    }
    
    pub fn automaton_not_found(id: &str) -> Error {
        error!(ErrorCode::AutomatonNotFound, "Automaton not found: {}", id)
    }
    
    pub fn invalid_parameter_value(param: &str, value: f64, min: f64, max: f64) -> Error {
        error!(
            ErrorCode::InvalidParameterValue,
            "Invalid value for parameter {}: {} (allowed range: {} - {})",
            param, value, min, max
        )
    }
}

/// Ошибки конфигурации
pub mod config {
    use super::*;
    
    pub fn not_found(path: &str) -> Error {
        error!(ErrorCode::ConfigNotFound, "Configuration not found: {}", path)
    }
    
    pub fn invalid_format(details: &str) -> Error {
        error!(ErrorCode::InvalidConfigFormat, "Invalid configuration format: {}", details)
    }
    
    pub fn missing_field(field: &str) -> Error {
        error!(ErrorCode::MissingField, "Missing required field: {}", field)
    }
}

/// Ошибки времени выполнения
pub mod runtime {
    use super::*;
    
    pub fn realtime_violation(details: &str) -> Error {
        error!(ErrorCode::RealtimeViolation, "Real-time violation: {}", details)
    }
    
    pub fn priority_error(details: &str) -> Error {
        error!(ErrorCode::PriorityError, "Failed to set thread priority: {}", details)
    }
    
    pub fn already_running() -> Error {
        Error::new(ErrorCode::AlreadyRunning, "Already running")
    }
    
    pub fn not_running() -> Error {
        Error::new(ErrorCode::NotRunning, "Not running")
    }
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::NodeId;
    
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
        let err = Error::new(ErrorCode::BufferFull, "Main error")
            .with_cause(cause);
        
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
        let err = buffer::full();
        assert_eq!(err.code, ErrorCode::BufferFull);
        
        let err = graph::node_not_found(NodeId(42));
        assert_eq!(err.code, ErrorCode::NodeNotFound);
        assert!(err.message.contains("42"));
        
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
        assert_eq!(ErrorCode::RealtimeViolation.category(), ErrorCategory::Runtime);
    }
    
    #[test]
    fn test_realtime_critical() {
        assert!(buffer::full().is_realtime_critical());
        assert!(io::xrun().is_realtime_critical());
        assert!(runtime::realtime_violation("test").is_realtime_critical());
        
        assert!(!graph::node_not_found(NodeId(1)).is_realtime_critical());
        assert!(!config::not_found("test").is_realtime_critical());
    }
}