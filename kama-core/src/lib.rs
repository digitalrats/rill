//! Kama Core - библиотека для создания аудиосистем

#![warn(missing_docs)]

pub mod buffer;
pub mod node;
pub mod graph;
pub mod automation;
pub mod param;
pub mod signal;
pub mod sequencer;
pub mod synth;
pub mod dsp;
pub mod util;
pub mod control;
pub mod mixer;

// Re-exports для удобства
// Убираем несуществующие типы
pub use automation::Automaton;
pub use graph::{AudioGraph, NodeId, PortId, Connection};
pub use node::{NodeFactory, NodeMetadata, NodeCategory};
pub use param::{ParamValue, ParamType, ParamRange};

// Правильно экспортируем AudioNode
pub use crate::node::AudioNode;

// Типы для аудио
pub type AudioBuffer = Vec<f32>;
pub type AudioResult<T> = Result<T, AudioError>;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("Audio processing error: {0}")]
    Processing(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Parameter error: {0}")]
    Parameter(String),
    
    #[error("Graph error: {0}")]
    Graph(String),
    
    #[error("MIDI error: {0}")]
    Midi(String),
    
    #[error("Signal error: {0}")]
    Signal(String),
}