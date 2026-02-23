//! Базовые трейты экосистемы Kama Audio

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
    pub use super::node::{AudioNode, NodeId};
    pub use super::param::{ParamValue, ParameterId, ParamType, ParamMetadata};  // добавили ParameterId
    pub use super::time::{Clock, TimeProvider};
}