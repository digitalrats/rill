//! # Математические абстракции для аудиообработки
//!
//! Этот модуль предоставляет:
//! - `AudioNum` — обобщенный числовой трейт для f32/f64
//! - Общие математические функции (lerp, db conversion, и т.д.)
//! - Быстрые аппроксимации для DSP

mod conversions;
mod functions;
mod num;

pub use functions::*;
pub use num::AudioNum;
