//! Сигнальная система для коммуникации между компонентами
//!
//! Предоставляет:
//! - Трейт `Signal` для маркировки сигналов
//! - Готовые типы сигналов (`ParameterChanged`, `ClockTick`, `SystemEvent`)
//! - `SignalBus` для многопоточной передачи сигналов
//! - `SimpleSignalDispatcher` для синхронной диспетчеризации

#![warn(missing_docs)]

mod error;
mod types;
mod bus;
mod dispatcher;

// Делаем все модули публичными, чтобы их можно было использовать
pub use error::{SignalError, SignalResult};
pub use types::{
    Signal,
    SignalSource,
    ParameterChanged,
    ClockTick,
    SystemEvent,
};
pub use bus::{
    SignalBus,
    BusConfig,
    OverflowPolicy,
};
pub use dispatcher::{
    SimpleSignalDispatcher,
    SignalHandler,
    DynSignalHandler,
    SignalHandlerWrapper,
};