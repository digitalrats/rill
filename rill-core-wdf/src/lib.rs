//! Wave Digital Filter (WDF) core — elements, adapters, and analysis
//! for analog circuit modeling.
//!
//! All types are generic over [`rill_core::Transcendental`], supporting both `f32`
//! and `f64`. SIMD-accelerated batch processing is available via
//! `process_incident_vector` methods on [`Resistor`], [`Capacitor`],
//! [`Inductor`], and [`Diode`], plus the free function
//! [`elements::process_batch_simd`].
//!
//! ## Modules
//! - `macros` — WDF eDSL macros for defining elements and filters
//! - `analysis` — frequency response and distortion analysis
//! - `constants` — physical constants and tolerances
//! - `filters` — WDF-based filter models
//!
//! # Design
//!
//! WDF elements are built around the [`WdfElement`] trait, which defines a
//! port resistance, wave processing, and state update cycle. Elements can be
//! combined via [`SeriesAdapter`] and [`ParallelAdapter`] to form arbitrary
//! linear circuits. Nonlinear elements like [`Diode`] use Newton-Raphson
//! iteration for implicit solution.
//!
//! References:
//! - A. Fettweis, "Wave Digital Filters: Theory and Practice" (1986)
//! - K. J. Werner et al., "An Improved and Generalized Diode Clipper
//!   Model for Wave Digital Filters" (2015)

#![warn(missing_docs)]
#![deny(unsafe_code)]

pub use rill_core::Transcendental;

/// WDF eDSL macros for defining elements and filters
pub mod macros;

mod adapters;
/// Frequency response and distortion analysis
pub mod analysis;
/// Physical constants and tolerances
pub mod constants;
mod elements;
/// Analog tape recording and playback head models
pub mod tape;

/// WDF-based filter models
pub mod filters;

pub use adapters::{ParallelAdapter, SeriesAdapter};
pub use elements::{Capacitor, Diode, Inductor, OpAmp, Resistor};

/// Base WDF element trait.
///
/// Every WDF element has a port resistance and processes incident
/// waves to produce reflected waves. For SIMD batch processing,
/// see the `process_incident_vector` methods on concrete element types.
pub trait WdfElement<T: Transcendental>: Send + Sync {
    /// Port resistance
    fn port_resistance(&self) -> T;

    /// Process incident wave, return reflected wave
    fn process_incident(&mut self, a: T) -> T;

    /// Update internal state (called after wave computation)
    fn update_state(&mut self);

    /// Current voltage across the element
    fn voltage(&self) -> T;

    /// Current current through the element
    fn current(&self) -> T;

    /// Reset to initial state
    fn reset(&mut self);
}

/// Wave variables: a (incident), b (reflected)
#[derive(Debug, Clone, Copy)]
pub struct WaveVariables<T: Transcendental> {
    /// Incident wave
    pub a: T,
    /// Reflected wave
    pub b: T,
}

impl<T: Transcendental> WaveVariables<T> {
    /// Create zero wave variables
    pub fn new() -> Self {
        Self {
            a: T::ZERO,
            b: T::ZERO,
        }
    }

    /// Compute voltage and current from wave variables
    pub fn to_voltage_current(&self, port_resistance: T) -> (T, T) {
        let two = T::from_f32(2.0);
        let v = (self.a + self.b) / two;
        let i = (self.a - self.b) / (two * port_resistance);
        (v, i)
    }

    /// Compute wave variables from voltage and current
    pub fn from_voltage_current(v: T, i: T, port_resistance: T) -> Self {
        let a = v + port_resistance * i;
        let b = v - port_resistance * i;
        Self { a, b }
    }
}

impl<T: Transcendental> Default for WaveVariables<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wave_variables() {
        let wv: WaveVariables<f64> = WaveVariables::new();
        assert_eq!(wv.a, 0.0);
        assert_eq!(wv.b, 0.0);
    }

    #[test]
    fn test_wave_to_voltage_current() {
        let wv: WaveVariables<f64> = WaveVariables { a: 2.0, b: 0.5 };
        let (v, i) = wv.to_voltage_current(100.0);
        assert!((v - 1.25).abs() < 1e-10);
        assert!((i - 0.0075).abs() < 1e-10);
    }

    #[test]
    fn test_voltage_current_to_wave() {
        let wv: WaveVariables<f64> = WaveVariables::from_voltage_current(1.0, 0.01, 100.0);
        assert!((wv.a - 2.0).abs() < 1e-10);
        assert!((wv.b - 0.0).abs() < 1e-10);
    }
}
