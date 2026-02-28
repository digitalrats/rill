//! # Kama Control - ядро системы маппинга
//!
//! Этот крейт предоставляет чистую логику для маппинга событий контроллеров
//! на параметры аудиоузлов. Он не зависит от конкретных источников событий
//! (MIDI, OSC, HID) и может использоваться с любыми входными данными.
//!
//! ## Основные компоненты
//!
//! - [`ControlEvent`] - абстрактное событие контроллера
//! - [`Mapping`] - связь между событием и параметром
//! - [`ControlEngine`] - основной движок, применяющий маппинги
//! - [`ControlNode`] - опциональная интеграция с AudioGraph
//!
//! ## Пример использования
//!
//! ```no_run
//! use kama_control::{ControlEngine, Mapping, Target, Transform, ControlEvent};
//! use kama_core::traits::{NodeId, ParameterId, PortId};
//! use crossbeam_channel::unbounded;
//!
//! // Создаём движок
//! let mut engine = ControlEngine::new();
//!
//! // Настраиваем канал для отправки изменений
//! let (tx, rx) = unbounded();
//! engine.set_output_channel(tx);
//!
//! // Создаём маппинг
//! let node = NodeId(0);
//! let port = PortId::control_in(node, 0);
//! let param = ParameterId::new("gain").unwrap();
//! let target = Target::new(port, param, 0.0, 1.0);
//!
//! let mapping = Mapping::new(
//!     EventPattern::Knob(7),
//!     target,
//!     Transform::Linear,
//! );
//!
//! engine.add_mapping(mapping);
//!
//! // Обрабатываем событие
//! let event = ControlEvent::Knob { id: 7, value: 0.5 };
//! engine.process_event(event);
//!
//! // Изменение параметра будет отправлено в канал
//! ```

#![warn(missing_docs)]

mod error;
mod mapping;
mod engine;

#[cfg(feature = "graph")]
mod node;

pub use error::{ControlError, ControlResult};
pub use mapping::{
    ControlEvent,
    EventPattern,
    Mapping,
    Target,
    Transform,
};

pub use engine::{ControlEngine, ControlStats};

#[cfg(feature = "graph")]
pub use node::ControlNode;