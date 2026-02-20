// kama-automation/src/lib.rs
//! Kama Automation - продвинутая система автоматизации
//!
//! Этот крейт предоставляет:
//! - Автоматы (LFO, огибающие)
//! - Сервоприводы для управления параметрами
//! - Менеджер автоматизации

#![warn(missing_docs)]

pub mod error;
pub mod context;
pub mod parameter;
pub mod automaton;
pub mod servo;
pub mod manager;
pub mod signal;
pub mod parameter_auto;

// Реэкспорт основных типов
pub use error::{AutomationError, AutomationResult};
pub use context::AutomationContext;
pub use parameter::{ParameterMap, ParameterData};
pub use automaton::{
    Automaton,
    LfoAutomaton, LfoAction, LfoState,
    EnvelopeState, EnvelopeStage,
};
pub use servo::{Servo, AnyServo, ParameterMapping};
pub use manager::AutomationManager;
pub use signal::{SignalSender, TestSignalSender};
pub use parameter_auto::AutomatedParameter;

// Правильный импорт из kama-signal (без вложенного модуля signal)
pub use kama_signal::{Signal, SignalHandler, ParameterChanged, SignalSource};