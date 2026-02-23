//! Сигнальная система Kama Audio
//!
//! Предоставляет инструменты для коммуникации между компонентами:
//!
//! ## Основные компоненты
//!
//! - **SignalBus** — многопоточные шины для передачи сигналов
//! - **ParameterChanged** — сигнал об изменении параметра порта
//! - **SystemEvent** — системные события (граф изменён, транспорт и т.д.)
//! - **SimpleSignalDispatcher** — синхронная диспетчеризация сигналов
//!
//! ## Пример использования
//!
//! ```rust
//! use kama_core::signal::*;
//! use kama_core::traits::{NodeId, ParameterId, PortId};
//!
//! // Создаём шину для сигналов изменения параметров
//! let bus = SignalBus::<ParameterChanged>::new(BusConfig::Unbounded);
//! let receiver = bus.receiver();
//!
//! // Создаём идентификаторы
//! let node = NodeId(42);
//! let port = PortId::node(node);
//! let param = ParameterId::new("frequency").unwrap();
//!
//! // Отправляем сигнал
//! let signal = ParameterChanged::new(
//!     port,
//!     param,
//!     440.0,
//!     0.5,
//!     SignalSource::Automation,
//! );
//!
//! bus.send(signal).unwrap();
//! ```

mod bus;
mod dispatcher;
mod types;
mod error;

#[cfg(feature = "serde")]
mod serde_impl;

pub use bus::*;
pub use dispatcher::*;
pub use types::*;
pub use error::*;

/// Прелюдия для удобного импорта основных типов
pub mod prelude {
    pub use super::bus::{SignalBus, BusConfig, OverflowPolicy};
    pub use super::types::{ParameterChanged, SystemEvent, SignalSource};
    pub use super::dispatcher::SimpleSignalDispatcher;
}