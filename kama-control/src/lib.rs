//! # Управление контроллерами для Kama Audio
//!
//! Этот крейт предоставляет унифицированный интерфейс для работы с различными
//! типами контроллеров (MIDI, HID, OSC) и их интеграцию с аудиографом через [`ControlNode`].
//!
//! ## Основные компоненты
//!
//! - **Бэкенды** — реализуют [`ControlBackend`] для конкретных типов устройств
//! - **События** — [`ControlEvent`] представляет все типы входящих сообщений
//! - **Маппинги** — [`Mapping`] связывает события с параметрами узлов
//! - **Узел управления** — [`ControlNode`] обрабатывает события и применяет маппинги
//!
//! ## Пример использования
//!
//! ```no_run
//! use kama_control::{ControlBackend, ControlNode, Mapping, Target, Transform};
//! use kama_control::backends::midi::MidiBackend;
//! use kama_core_traits::NodeId;  // <-- ДОБАВЛЕН ЭТОТ ИМПОРТ
//!
//! // Создаём MIDI бэкенд
//! let mut midi = MidiBackend::new("MyApp").unwrap();
//! midi.open_port(0).unwrap();
//!
//! // Создаём узел управления
//! let event_rx = midi.subscribe();
//! let mut control_node = ControlNode::new(event_rx);
//!
//! // Маппим MIDI контроллер 7 на громкость
//! use kama_control::EventPattern;
//! control_node.add_mapping(Mapping::new(
//!     EventPattern::MidiControl { channel: None, controller: 7 },
//!     Target { node_id: NodeId(0), param_name: "gain".to_string(), min: 0.0, max: 1.0 },
//!     Transform::Exponential,
//! ));
//! ```
//! 
//! [`ControlNode`]: crate::node::ControlNode
//! [`ControlBackend`]: crate::backend::ControlBackend
//! [`ControlEvent`]: crate::backend::ControlEvent
//! [`Mapping`]: crate::mapping::Mapping

// Control backends for Kama Audio - MIDI, HID, OSC, Mackie
//! 
//! Этот крейт предоставляет унифицированный интерфейс для различных
//! контроллеров и их интеграцию с AudioGraph через ControlNode.

#![warn(missing_docs)]

mod backend;
mod mapping;
mod node;
mod error;

pub mod backends;

pub use backend::{ControlBackend, BackendType, DeviceInfo, ControlEvent};
pub use mapping::{Mapping, Target, Transform, EventPattern};
pub use node::ControlNode;
pub use error::{ControlError, ControlResult};

// Реэкспорты из kama-core-traits для удобства
pub use kama_core_traits::{NodeId, AudioNode, ParamValue};

/// Преобразует MIDI сообщение в ControlEvent
pub fn midi_to_event(message: &[u8]) -> Option<ControlEvent> {
    if message.len() < 3 {
        return None;
    }
    
    let status = message[0];
    let channel = status & 0x0F;
    let msg_type = status & 0xF0;
    
    match msg_type {
        0x80 => { // Note Off
            Some(ControlEvent::MidiNote {
                channel,
                note: message[1],
                velocity: 0,
                on: false,
            })
        }
        0x90 => { // Note On
            Some(ControlEvent::MidiNote {
                channel,
                note: message[1],
                velocity: message[2],
                on: message[2] > 0,
            })
        }
        0xB0 => { // Control Change
            Some(ControlEvent::MidiControl {
                channel,
                controller: message[1],
                value: message[2],
                normalized: message[2] as f32 / 127.0,
            })
        }
        _ => {
            Some(ControlEvent::Midi {
                channel,
                message: message.to_vec(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_midi_to_event() {
        let msg = vec![0x90, 0x40, 0x7F]; // Note On, middle C, max velocity
        let event = midi_to_event(&msg).unwrap();
        
        match event {
            ControlEvent::MidiNote { channel, note, velocity, on } => {
                assert_eq!(channel, 0);
                assert_eq!(note, 0x40);
                assert_eq!(velocity, 0x7F);
                assert!(on);
            }
            _ => panic!("Wrong event type"),
        }
    }
    
    #[test]
    fn test_transform() {
        let linear = Transform::Linear;
        assert_eq!(linear.apply(0.5, 0.0, 10.0), 5.0);
        
        let exp = Transform::Exponential;
        assert!((exp.apply(0.5, 0.0, 10.0) - 2.5).abs() < 0.001);
        
        let inv = Transform::Inverted;
        assert_eq!(inv.apply(0.3, 0.0, 10.0), 7.0);
    }
}