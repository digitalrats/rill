//! # Rill Patchbay — Маршрутизация событий и автоматизация
//!
//! `rill-patchbay` является эволюцией `rill-automation` из версии 0.2.0,
//! объединённой с функциональностью маппинга из `rill-control`.
//!
//! ## Основные компоненты
//!
//! - **Автоматы** — генеративные источники сигналов (LFO, огибающие, секвенсоры)
//! - **Сервоприводы** (в модуле `control`) — связь автоматов с параметрами узлов
//! - **Маппинги** — связь внешних событий (MIDI/OSC) с параметрами
//! - **Сенсоры** — источники событий из внешнего мира
//! - **Менеджер** — центральный координатор для двухпоточной архитектуры
//!
//! ## Архитектура
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     ПОТОК УПРАВЛЕНИЯ                         │
//! │                                                              │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │               Manager                         │   │
//! │  │  ┌────────────┐  ┌────────────┐  ┌────────────┐     │   │
//! │  │  │  Automata  │  │  Servos    │  │  Mappings  │     │   │
//! │  │  └────────────┘  └────────────┘  └────────────┘     │   │
//! │  │                    │                │                │   │
//! │  │                    ▼                ▼                │   │
//! │  │              ┌──────────────────────────┐           │   │
//! │  │              │   RtQueue<ParameterCommand>│         │   │
//! │  │              └──────────────────────────┘           │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! │                              │                               │
//! │                              │ неблокирующая очередь         │
//! │                              ▼                               │
//! │  ┌─────────────────────────────────────────────────────┐   │
//! │  │                  АУДИОПОТОК                          │   │
//! │  │              (rill-graph / rill-io)                  │   │
//! │  └─────────────────────────────────────────────────────┘   │
//! └─────────────────────────────────────────────────────────────┘
//! ```

#![warn(missing_docs)]
#![deny(unsafe_code)]
#![allow(clippy::too_many_arguments)]

// =============================================================================
// Внешние зависимости
// =============================================================================

// Реэкспорты из rill-core
pub use rill_core::prelude::*;
pub use rill_core::queues::RtQueue;
pub use rill_core::{NodeId, ParamValue, ParameterId, PortId};

// =============================================================================
// Публичные модули
// =============================================================================

/// Автоматы — генеративные источники управления
pub mod automaton;

/// Управление и маппинг событий
pub mod engine;

/// Менеджер патчбэя — центральный координатор
pub mod manager;

/// Сенсоры — источники событий из внешнего мира
pub mod sensor;

/// Утилиты и вспомогательные функции
pub mod utils;

/// Реестр именованных функций для сериализации
pub mod function_registry;

/// Стратегии управления автоматами
pub mod strategy;

/// PortCombiner — комбинирование автомата и UI на порт
pub mod port_combiner;

/// Обёртка Automaton в green thread (tokio task)
pub mod automaton_task;

/// Parameter-lock step sequencer
pub mod sequencer;

/// DOT patchbay visualization (Graphviz)
#[cfg(feature = "serde")]
pub mod dot;

/// Сериализация конфигурации управления
#[cfg(feature = "serde")]
pub mod document;

#[cfg(feature = "serde")]
pub use document::PatchbayDocument;

// =============================================================================
// Реэкспорты для удобства
// =============================================================================

// Selective re-exports
pub use automaton::{
    EnvelopeAutomaton, EnvelopeStage, EnvelopeType, FunctionAutomaton, LfoAutomaton, LfoWaveform,
    PlayMode, Range, SequencerAutomaton, StatefulFunctionAutomaton, Step, SyncMode,
};
pub use automaton_task::spawn_automaton_task;
pub use engine::{
    midi_cc, osc_address, AnyServo, Automaton, BoxedServo, ControlEvent, Engine, EventPattern,
    Mapping, NoAction, OscSurface, OscSurfaceEntry, ParameterMapping, Servo, Target, Transform,
};

pub use manager::Manager;
pub use port_combiner::{spawn_combiner, PortCombinerHandle};
pub use strategy::{ConflictStrategy, ControlStrategy, UiCommand};

// Sequencer re-exports
#[cfg(feature = "serde")]
pub use sequencer::SequencerDocument;
pub use sequencer::{
    ParameterTarget, Pattern, SequenceStep, SequencerHandle, Snapshot, SnapshotSequencer,
    StepPlayMode,
};

// =============================================================================
// Прелюдия для удобного импорта
// =============================================================================

/// Прелюдия для удобного импорта основных типов
pub mod prelude {
    // Основные типы
    pub use crate::automaton::*;
    pub use crate::automaton_task::*;
    pub use crate::engine::*;
    pub use crate::manager::*;
    pub use crate::port_combiner::*;
    pub use crate::sequencer::*;
    pub use crate::strategy::*;
    pub use crate::utils::*;

    // Реэкспорты из rill-core
    pub use rill_core::prelude::*;
    pub use rill_core::queues::RtQueue;
    pub use rill_core::{NodeId, ParameterId, PortId};
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_imports() {
        // Просто проверяем, что всё импортируется
        let _ = automaton::LfoWaveform::Sine;
        let _ = engine::Transform::Linear;
        let _ = manager::PatchbayConfig::default();
    }
}
