//! Сигнальная система для коммуникации между компонентами
//!
//! Предоставляет:
//! - Трейт `Signal` для маркировки сигналов
//! - Готовые типы сигналов (`ParameterChanged`, `ClockTick`, `SystemEvent`)
//! - `SignalBus` для многопоточной передачи сигналов
//! - `SimpleSignalDispatcher` для синхронной диспетчеризации

#![warn(missing_docs)]

mod bus;
mod dispatcher;
mod error;
mod types;

// Делаем все модули публичными, чтобы их можно было использовать
pub use bus::{BusConfig, OverflowPolicy, SignalBus};
pub use dispatcher::{
    DynSignalHandler, SignalHandler, SignalHandlerWrapper, SimpleSignalDispatcher,
};
pub use error::{SignalError, SignalResult};
pub use types::{ClockTick, ParameterChanged, Signal, SignalSource, SystemEvent};
