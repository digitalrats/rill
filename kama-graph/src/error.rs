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
    Audio(#[from] kama_core_traits::AudioError),
    
    #[error("Buffer error: {0}")]
    Buffer(String),
    
    #[error("Cycle detected in graph")]
    CycleDetected,
}

/// Результат операций с графом
pub type GraphResult<T> = Result<T, GraphError>;