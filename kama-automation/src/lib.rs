// kama-automation/src/lib.rs
//! Kama Automation - продвинутая система автоматизации

#![warn(missing_docs)]

pub mod automaton;
pub mod context;
pub mod error;
pub mod manager;
pub mod parameter;
pub mod parameter_auto;
pub mod servo;
pub mod signal;

// Реэкспорт основных типов
pub use automaton::{
    Automaton,
    FunctionAutomaton,
    LfoAutomaton,
    LfoWithEnvelopeAutomaton, // Эти типы теперь доступны
    StatefulFunctionAutomaton,
    Waveform,
};
pub use context::AutomationContext;
pub use error::{AutomationError, AutomationResult};
pub use manager::{AutomationManager, DefaultAutomationManager};
pub use parameter::{ParameterData, ParameterMap};
pub use parameter_auto::AutomatedParameter;
pub use servo::{AnyServo, ParameterMapping, Servo};
pub use signal::{SignalSender, TestSignalSender};
