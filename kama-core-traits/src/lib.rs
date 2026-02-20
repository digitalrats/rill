//! Базовые трейты для экосистемы Kama Audio
//!
//! Этот крейт содержит только трейты и базовые типы, без реализаций.
//! Это позволяет избежать циклических зависимостей между крейтами.

#![warn(missing_docs)]

pub mod error;
pub mod param;
pub mod node;
pub mod time;

// Реэкспорты для удобства
pub use error::{AudioError, AudioResult};
pub use param::{ParamValue, ParamType, ParamRange, ParamMetadata};
pub use node::{
    AudioNode, NodeCategory, NodeMetadata, NodeCreator, NodeTypeId,
    NodeId, PortId,
};
pub use time::{Clock, TimeProvider, TickInfo};