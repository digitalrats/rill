//! Control backends for Kama Audio - MIDI, HID, OSC, Mackie
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