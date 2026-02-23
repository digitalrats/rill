use thiserror::Error;

/// Ошибки графа обработки
#[derive(Error, Debug)]
pub enum GraphError {
    #[error("Invalid node ID")]
    InvalidNodeId,

    #[error("Invalid port")]
    InvalidPort,

    #[error("Invalid connection direction")]
    InvalidConnectionDirection,

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("Audio error: {0}")]
    Audio(#[from] kama_core::traits::AudioError),

    #[error("Buffer error: {0}")]
    Buffer(String),

    #[error("Cycle detected in graph")]
    CycleDetected,
}

/// Результат операций с графом
pub type GraphResult<T> = Result<T, GraphError>;

/// Ошибки реестра узлов
#[derive(Error, Debug)] // <-- ДОБАВЛЕНО
pub enum RegistryError {
    #[error("Node type '{0}' not found")]
    NodeTypeNotFound(String),

    #[error("Node type '{0}' already registered")]
    NodeTypeAlreadyRegistered(String),

    #[error("Failed to create node: {0}")]
    CreationFailed(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),

    #[error("Graph error: {0}")]
    Graph(String),
}

/// Результат операций с реестром
pub type RegistryResult<T> = Result<T, RegistryError>;
