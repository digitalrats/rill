//! Wave Digital Filter (WDF) core — elements, adapters, and analysis
//! for analog circuit modeling.
//!
//! References:
//! - A. Fettweis, "Wave Digital Filters: Theory and Practice" (1986)
//! - K. J. Werner et al., "An Improved and Generalized Diode Clipper
//!   Model for Wave Digital Filters" (2015)

#![warn(missing_docs)]
#![deny(unsafe_code)]

mod elements;
mod adapters;
/// Frequency response and distortion analysis
pub mod analysis;

#[cfg(feature = "simd")]
pub mod simd;

pub use elements::{Resistor, Capacitor, Inductor, Diode};
pub use adapters::{SeriesAdapter, ParallelAdapter};

/// Wave port type for WDF adapters
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortType {
    /// Series connection
    Series,
    /// Parallel connection
    Parallel,
    /// Reflection port
    Reflection,
}

/// Wave variables: a (incident), b (reflected)
#[derive(Debug, Clone, Copy)]
pub struct WaveVariables {
    /// Incident wave
    pub a: f64,
    /// Reflected wave
    pub b: f64,
}

impl WaveVariables {
    /// Create zero wave variables
    pub fn new() -> Self {
        Self { a: 0.0, b: 0.0 }
    }

    /// Compute voltage and current from wave variables
    pub fn to_voltage_current(&self, port_resistance: f64) -> (f64, f64) {
        let v = (self.a + self.b) / 2.0;
        let i = (self.a - self.b) / (2.0 * port_resistance);
        (v, i)
    }

    /// Compute wave variables from voltage and current
    pub fn from_voltage_current(v: f64, i: f64, port_resistance: f64) -> Self {
        let a = v + port_resistance * i;
        let b = v - port_resistance * i;
        Self { a, b }
    }
}

impl Default for WaveVariables {
    fn default() -> Self {
        Self::new()
    }
}

/// Base WDF element trait
///
/// Every WDF element has a port resistance and processes incident
/// waves to produce reflected waves.
pub trait WdfElement: Send + Sync {
    /// Port resistance
    fn port_resistance(&self) -> f64;

    /// Process incident wave, return reflected wave
    fn process_incident(&mut self, a: f64) -> f64;

    /// Update internal state (called after wave computation)
    fn update_state(&mut self);

    /// Current voltage across the element
    fn voltage(&self) -> f64;

    /// Current current through the element
    fn current(&self) -> f64;

    /// Reset to initial state
    fn reset(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wave_variables() {
        let wv = WaveVariables::new();
        assert_eq!(wv.a, 0.0);
        assert_eq!(wv.b, 0.0);
    }

    #[test]
    fn test_wave_to_voltage_current() {
        let wv = WaveVariables { a: 2.0, b: 0.5 };
        let (v, i) = wv.to_voltage_current(100.0);
        assert!((v - 1.25).abs() < 1e-10);
        assert!((i - 0.0075).abs() < 1e-10);
    }

    #[test]
    fn test_voltage_current_to_wave() {
        let wv = WaveVariables::from_voltage_current(1.0, 0.01, 100.0);
        assert!((wv.a - 2.0).abs() < 1e-10);
        assert!((wv.b - 0.0).abs() < 1e-10);
    }
}
