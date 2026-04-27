//! SIMD-accelerated WDF elements
//!
//! Provides vectorized implementations of WDF elements using
//! `core::simd` (stable since Rust 1.78).
//!
//! # Safety
//!
//! This module is safe code. It uses `core::simd` portable SIMD types
//! and avoids unsafe alignment tricks.

use core::simd::{f64x4, SimdFloat};

/// SIMD WDF element trait
pub trait SimdWdfElement: Send + Sync {
    /// SIMD vector type for this element
    type SimdType: SimdFloat;

    /// Process incident wave SIMD vector, return reflected wave
    fn process_incident_simd(&mut self, a: Self::SimdType) -> Self::SimdType;

    /// Update state for SIMD
    fn update_state_simd(&mut self);

    /// Get SIMD voltage vector
    fn voltage_simd(&self) -> Self::SimdType;

    /// Get SIMD current vector
    fn current_simd(&self) -> Self::SimdType;
}

/// SIMD-accelerated resistor
#[derive(Debug, Clone)]
pub struct SimdResistor {
    resistance: f64,
    port_resistance: f64x4,
    voltage: f64x4,
    current: f64x4,
}

impl SimdResistor {
    /// Create a new SIMD resistor
    pub fn new(resistance: f64) -> Self {
        Self {
            resistance,
            port_resistance: f64x4::splat(resistance),
            voltage: f64x4::splat(0.0),
            current: f64x4::splat(0.0),
        }
    }
}

impl SimdWdfElement for SimdResistor {
    type SimdType = f64x4;

    fn process_incident_simd(&mut self, _a: f64x4) -> f64x4 {
        f64x4::splat(0.0)
    }

    fn update_state_simd(&mut self) {
        self.voltage = self.current * self.port_resistance;
    }

    fn voltage_simd(&self) -> f64x4 {
        self.voltage
    }

    fn current_simd(&self) -> f64x4 {
        self.current
    }
}

/// SIMD-accelerated capacitor
#[derive(Debug, Clone)]
pub struct SimdCapacitor {
    capacitance: f64,
    sample_rate: f64,
    port_resistance: f64x4,
    state: f64x4,
    dt: f64,
}

impl SimdCapacitor {
    /// Create a new SIMD capacitor
    pub fn new(capacitance: f64, sample_rate: f64) -> Self {
        let t = 1.0 / sample_rate;
        let port_resistance = t / (2.0 * capacitance);

        Self {
            capacitance,
            sample_rate,
            port_resistance: f64x4::splat(port_resistance),
            state: f64x4::splat(0.0),
            dt: t,
        }
    }
}

impl SimdWdfElement for SimdCapacitor {
    type SimdType = f64x4;

    fn process_incident_simd(&mut self, a: f64x4) -> f64x4 {
        self.state - a
    }

    fn update_state_simd(&mut self) {
        let current = -self.state / self.port_resistance;
        self.state = -current * self.port_resistance;
    }

    fn voltage_simd(&self) -> f64x4 {
        self.state
    }

    fn current_simd(&self) -> f64x4 {
        -self.state / self.port_resistance
    }
}

/// SIMD-accelerated diode with vectorized Newton-Raphson
#[derive(Debug, Clone)]
pub struct SimdDiode {
    saturation_current: f64,
    thermal_voltage: f64,
    ideality_factor: f64,
    port_resistance: f64x4,
    vt_simd: f64x4,
    is_simd: f64x4,
    tolerance_simd: f64x4,
}

impl SimdDiode {
    /// Create a new SIMD diode
    pub fn new(saturation_current: f64, ideality_factor: f64, temperature_k: f64) -> Self {
        let k = 1.380649e-23;
        let q = 1.60217662e-19;
        let thermal_voltage = (k * temperature_k) / q;
        let vt = thermal_voltage * ideality_factor;

        Self {
            saturation_current,
            thermal_voltage,
            ideality_factor,
            port_resistance: f64x4::splat(vt / saturation_current),
            vt_simd: f64x4::splat(vt),
            is_simd: f64x4::splat(saturation_current),
            tolerance_simd: f64x4::splat(1e-9),
        }
    }

    fn solve_newton_simd(&self, a: f64x4, r: f64x4) -> f64x4 {
        let mut v = f64x4::splat(0.0);

        for _ in 0..6 {
            let i = self.is_simd * ((v / self.vt_simd).exp() - f64x4::splat(1.0));
            let g = self.is_simd * (v / self.vt_simd).exp() / self.vt_simd;

            let f = v + r * i - a;

            let converged = f.abs() < self.tolerance_simd;
            if converged.all() {
                break;
            }

            let df = f64x4::splat(1.0) + r * g;
            v = v - f / df;
        }

        v
    }
}

impl SimdWdfElement for SimdDiode {
    type SimdType = f64x4;

    fn process_incident_simd(&mut self, a: f64x4) -> f64x4 {
        let v = self.solve_newton_simd(a, self.port_resistance);
        let _i = self.is_simd * ((v / self.vt_simd).exp() - f64x4::splat(1.0));

        f64x4::splat(2.0) * v - a
    }

    fn update_state_simd(&mut self) {}

    fn voltage_simd(&self) -> f64x4 {
        f64x4::splat(0.0)
    }

    fn current_simd(&self) -> f64x4 {
        f64x4::splat(0.0)
    }
}

/// Process a batch of samples through a SIMD WDF element
pub fn process_batch_simd(
    element: &mut dyn SimdWdfElement<SimdType = f64x4>,
    inputs: &[f64],
    outputs: &mut [f64],
) {
    let len = inputs.len().min(outputs.len());
    let chunks = len / 4;
    let remainder = len % 4;

    for i in 0..chunks {
        let offset = i * 4;
        let a = f64x4::from_slice(&inputs[offset..offset + 4]);
        let b = element.process_incident_simd(a);
        b.copy_to_slice(&mut outputs[offset..offset + 4]);
    }

    if remainder > 0 {
        let offset = chunks * 4;
        let mut tail = [0.0f64; 4];
        tail[..remainder].copy_from_slice(&inputs[offset..offset + remainder]);
        let a = f64x4::from_array(tail);
        let b = element.process_incident_simd(a);
        let b_arr: [f64; 4] = b.into();
        outputs[offset..offset + remainder].copy_from_slice(&b_arr[..remainder]);
    }
}
