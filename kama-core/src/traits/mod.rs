//! Базовые трейты экосистемы Kama Audio
//!
//! Этот модуль содержит все системообразующие трейты:
//! - `AudioNode` — базовый узел обработки
//! - `ParamValue` — типизированные параметры
//! - `Clock`/`TimeProvider` — временные абстракции
//! - Типы ошибок

mod error;
mod node;
mod param;
pub mod time;

pub use error::*;
pub use node::*;
pub use param::*;
pub use time::*;

/// Прелюдия для удобного импорта основных трейтов
pub mod prelude {
    pub use super::error::AudioResult;
    pub use super::node::AudioNode;
    pub use super::param::ParamValue;
    pub use super::time::{Clock, TimeProvider};
    
    // Если есть идентификаторы, добавьте их
    // pub use super::node::NodeId;
    // pub use super::node::PortId;
}