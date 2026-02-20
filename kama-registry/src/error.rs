use thiserror::Error;

/// Ошибки реестра узлов
#[derive(Error, Debug)]
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