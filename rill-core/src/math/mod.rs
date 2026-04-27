//! # Математические абстракции
//!
//! Этот модуль предоставляет:
//! - `Scalar` — базовый числовой трейт для любых типов (включая целые)
//! - `Transcendental` — расширение Scalar с тригонометрией (f32/f64)
//! - Общие математические функции (lerp, db conversion, и т.д.)
//! - Векторные операции через `vector` подмодуль
//! - Быстрые аппроксимации для DSP

mod conversions;
mod functions;
mod num;
pub mod vector;

pub use functions::*;
pub use num::{Scalar, Transcendental};
