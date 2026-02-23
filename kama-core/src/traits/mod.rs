//! Базовые трейты экосистемы Kama Audio

mod error;
mod node;
mod param;
mod port;  // новый модуль
pub mod time;

// Публично реэкспортируем всё из модулей
pub use error::*;
pub use node::*;
pub use param::*;
pub use port::*;
pub use time::*;

/// Прелюдия для удобного импорта основных трейтов
pub mod prelude {
    pub use super::error::AudioResult;
    pub use super::node::{AudioNode, NodeId, NodeMetadata, NodeCategory, NodeTypeId};
    pub use super::param::{ParamValue, ParamType, ParamMetadata};
    pub use super::port::PortId;
    pub use super::time::{Clock, TimeProvider, TickInfo};
}