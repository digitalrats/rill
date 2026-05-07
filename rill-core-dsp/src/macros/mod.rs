//! # Macros for creating DSP algorithms
//!
//! This module provides macros for conveniently creating DSP algorithms
//! that implement traits from `crate::algorithm` using `Transcendental` from `rill_core`.
//!
//! ## Available macros
//!
//! - `simple_algorithm!` - for simple algorithms without parameters
//! - `parameterized_algorithm!` - for algorithms with parameters
//! - `filter_algorithm!` - for filters (with coefficients)
//! - `effect_algorithm!` - for effects (with dry/wet)
//! - `generator_algorithm!` - for generators
//!
//! ## Example
//!
//! ```
//! use rill_core_dsp::simple_algorithm;
//! use rill_core::math::Transcendental;
//!
//! simple_algorithm! {
//!     /// Simple gain
//!     #[derive(Debug, Clone, Copy)]
//!     pub struct Gain<T: Transcendental> {
//!         params: {
//!     /// Gain coefficient
//!             gain: T = T::from_f32(1.0),
//!         },
//!         state: {
//!     /// Last output value (for statistics)
//!             last_output: T = T::ZERO,
//!         },
//!         process: |this, input| {
//!             let output = input * this.gain;
//!             this.last_output = output;
//!             output
//!         }
//!     }
//! }
//! ```

#[macro_use]
mod simple;
#[macro_use]
mod parameterized;
#[macro_use]
mod filter;
#[macro_use]
mod effect;
#[macro_use]
mod generator;

/// Prelude for convenient macro imports
pub mod prelude;
