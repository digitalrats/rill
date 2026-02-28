//! # Kama Patchbay - мир, где живут автоматы
//!
//! Этот крейт реализует систему управления для Kama Audio,
//! вдохновленную автоматами из вселенной Vorarlberg (игр Siberia).
//!
//! ## Концепция
//!
//! - **Automaton** - сущности, генерирующие и преобразующие сигналы
//! - **Sensor** - органы чувств (слышат звук, чувствуют прикосновения)
//! - **Servo** - исполнители (воздействуют на AudioGraph)
//! - **Patchbay** - мир, где все они живут
//!
//! ## Пример
//!
//! ```
//! use kama_patchbay::prelude::*;
//!
//! // Создаем мир
//! let mut world = Patchbay::new("MySynth");
//!
//! // Добавляем LFO автомат
//! world.create_lfo("vibrato");
//!
//! // Добавляем ручку, которая будет управлять частотой LFO
//! world.create_knob("vibrato_rate");
//!
//! // Добавляем сенсор, который слышит выход осциллятора
//! let pitch = AcousticSensor::new("pitch_detector", 
//!     Box::new(PitchDetector::new(44100.0)))
//!     .listening_to("osc1_out");
//! world.add_sensor(Box::new(pitch));
//!
//! // Запускаем мир
//! world.awaken();
//! ```

#![warn(missing_docs)]

pub mod core;
pub mod automaton;
pub mod sensor;
pub mod servo;
pub mod world;

/// Прелюдия для удобного импорта
pub mod prelude {
    pub use crate::core::*;
    pub use crate::automaton::*;
    pub use crate::sensor::*;
    pub use crate::servo::*;
    pub use crate::world::*;
}