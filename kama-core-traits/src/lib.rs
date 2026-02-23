//! Базовые трейты для экосистемы Kama Audio
//!
//! Этот крейт содержит только трейты и базовые типы, без реализаций.
//! Это позволяет избежать циклических зависимостей между крейтами.

#![warn(missing_docs)]

pub mod error;
pub mod node;
pub mod param;
pub mod time; // Модуль time содержит всё, что связано со временем

// Реэкспорты для удобства
pub use error::{AudioError, AudioResult};
pub use node::{AudioNode, NodeCategory, NodeCreator, NodeId, NodeMetadata, NodeTypeId, PortId};
pub use param::{ParamMetadata, ParamRange, ParamType, ParamValue};

// Реэкспорты из модуля time
pub use time::{Clock, SystemClock, TickInfo, TimeProvider};
