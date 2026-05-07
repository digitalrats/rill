//! # Mathematical abstractions
//!
//! This module provides:
//! - `Scalar` — base numeric trait for all types (including integers)
//! - `Transcendental` — Scalar extension with trigonometry (f32/f64)
//! - Common mathematical functions (lerp, db conversion, etc.)
//! - Vector operations through the `vector` submodule
//! - Fast approximations for DSP

mod conversions;
mod functions;
mod num;
pub mod vector;

pub use functions::*;
pub use num::{Scalar, Transcendental};
