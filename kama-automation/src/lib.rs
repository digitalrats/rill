// kama-automation/src/lib.rs
//! Kama Automation - продвинутая система автоматизации

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
    FunctionAutomaton, StatefulFunctionAutomaton,
    LfoAutomaton, LfoWithEnvelopeAutomaton,  // Эти типы теперь доступны
    Waveform,
};
pub use servo::{Servo, AnyServo, ParameterMapping};
pub use manager::{AutomationManager, DefaultAutomationManager};
pub use signal::{SignalSender, TestSignalSender};
pub use parameter_auto::AutomatedParameter;