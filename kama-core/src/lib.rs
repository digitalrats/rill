//! Kama Core - библиотека для создания аудиосистем

#![warn(missing_docs)]

// Re-export из kama-core-traits
pub use kama_core_traits::{
    AudioNode,
    AudioError,
    AudioResult,
    param::{ParamValue, ParamType, ParamRange},
    node::{NodeMetadata, NodeCategory, NodeCreator},
    time::{Clock, TimeProvider, TickInfo},
};

// Наши модули с реализациями
pub mod graph;
pub mod node;
pub mod param;
pub mod time;
pub mod dsp;
pub mod util;
pub mod control;
pub mod mixer;

// Re-exports для удобства
pub use graph::{AudioGraph, NodeId, PortId, Connection};
pub use node::{NodeFactory, GainNode, NodeRegistry};

// Типы для аудио
pub type AudioBuffer = Vec<f32>;