//! SIMD-accelerated WDF elements using rill-core vector infrastructure.
//!
//! Provides vectorized implementations of WDF elements using `Vector<f64, 4>`
//! (backed by `rill_core::vector::simd::F64x4`).

use crate::constants::{BOLTZMANN, ELECTRON_CHARGE, NEWTON_TOLERANCE};
use rill_core::vector::prelude::{F64x4, Vector, VectorMask, VectorTranscendental};

/// SIMD WDF element trait
pub trait SimdWdfElement: Send + Sync {
    /// SIMD vector type for this element (e.g. F64x4)
    type SimdType;

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
    #[allow(dead_code)]
    resistance: f64,
    port_resistance: F64x4,
    voltage: F64x4,
    current: F64x4,
}

impl SimdResistor {
    /// Create a new SIMD resistor
    pub fn new(resistance: f64) -> Self {
        Self {
            resistance,
            port_resistance: F64x4::splat(resistance),
            voltage: F64x4::splat(0.0),
            current: F64x4::splat(0.0),
        }
    }
}

impl SimdWdfElement for SimdResistor {
    type SimdType = F64x4;

    fn process_incident_simd(&mut self, _a: F64x4) -> F64x4 {
        F64x4::splat(0.0)
    }

    fn update_state_simd(&mut self) {
        self.voltage = self.current * self.port_resistance;
    }

    fn voltage_simd(&self) -> F64x4 {
        self.voltage
    }

    fn current_simd(&self) -> F64x4 {
        self.current
    }
}

/// SIMD-accelerated capacitor
#[derive(Debug, Clone)]
pub struct SimdCapacitor {
    #[allow(dead_code)]
    capacitance: f64,
    #[allow(dead_code)]
    sample_rate: f64,
    port_resistance: F64x4,
    state: F64x4,
    #[allow(dead_code)]
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
            port_resistance: F64x4::splat(port_resistance),
            state: F64x4::splat(0.0),
            dt: t,
        }
    }
}

impl SimdWdfElement for SimdCapacitor {
    type SimdType = F64x4;

    fn process_incident_simd(&mut self, a: F64x4) -> F64x4 {
        self.state - a
    }

    fn update_state_simd(&mut self) {
        let current = -self.state / self.port_resistance;
        self.state = -current * self.port_resistance;
    }

    fn voltage_simd(&self) -> F64x4 {
        self.state
    }

    fn current_simd(&self) -> F64x4 {
        -self.state / self.port_resistance
    }
}

/// SIMD-accelerated diode with vectorized Newton-Raphson
#[derive(Debug, Clone)]
pub struct SimdDiode {
    #[allow(dead_code)]
    saturation_current: f64,
    #[allow(dead_code)]
    thermal_voltage: f64,
    #[allow(dead_code)]
    ideality_factor: f64,
    port_resistance: F64x4,
    vt_simd: F64x4,
    is_simd: F64x4,
    tolerance_simd: F64x4,
}

impl SimdDiode {
    /// Create a new SIMD diode
    pub fn new(saturation_current: f64, ideality_factor: f64, temperature_k: f64) -> Self {
        let k = BOLTZMANN;
        let q = ELECTRON_CHARGE;
        let thermal_voltage = (k * temperature_k) / q;
        let vt = thermal_voltage * ideality_factor;

        Self {
            saturation_current,
            thermal_voltage,
            ideality_factor,
            port_resistance: F64x4::splat(vt / saturation_current),
            vt_simd: F64x4::splat(vt),
            is_simd: F64x4::splat(saturation_current),
            tolerance_simd: F64x4::splat(NEWTON_TOLERANCE),
        }
    }

    fn solve_newton_simd(&self, a: F64x4, r: F64x4) -> F64x4 {
        let mut v = F64x4::splat(0.0);

        for _ in 0..10 {
            let i = self.is_simd * ((v / self.vt_simd).exp() - F64x4::splat(1.0));
            let g = self.is_simd * (v / self.vt_simd).exp() / self.vt_simd;

            let f = v + r * i - a;

            let converged = <F64x4 as VectorMask<f64, 4>>::lt(&f.abs(), &self.tolerance_simd);
            if <F64x4 as VectorMask<f64, 4>>::all(&converged) {
                break;
            }

            let df = F64x4::splat(1.0) + r * g;
            v = v - f / df;
        }

        v
    }
}

impl SimdWdfElement for SimdDiode {
    type SimdType = F64x4;

    fn process_incident_simd(&mut self, a: F64x4) -> F64x4 {
        let v = self.solve_newton_simd(a, self.port_resistance);
        let _i = self.is_simd * ((v / self.vt_simd).exp() - F64x4::splat(1.0));

        F64x4::splat(2.0) * v - a
    }

    fn update_state_simd(&mut self) {}

    fn voltage_simd(&self) -> F64x4 {
        F64x4::splat(0.0)
    }

    fn current_simd(&self) -> F64x4 {
        F64x4::splat(0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simd_resistor() {
        let mut r = SimdResistor::new(1000.0);
        assert_eq!(r.port_resistance.extract(0), 1000.0);
        let b = r.process_incident_simd(F64x4::splat(1.0));
        assert!((b.extract(0) - 0.0).abs() < 1e-15);
    }

    #[test]
    fn test_simd_capacitor_port_resistance() {
        let sample_rate = 44100.0;
        let capacitance = 1e-6;
        let c = SimdCapacitor::new(capacitance, sample_rate);
        let expected_r = 1.0 / (sample_rate * 2.0 * capacitance);
        assert!((c.port_resistance.extract(0) - expected_r).abs() < 1e-12);
    }

    #[test]
    fn test_simd_capacitor_process_ident() {
        let sample_rate = 44100.0;
        let mut c = SimdCapacitor::new(1e-6, sample_rate);
        let b = c.process_incident_simd(F64x4::splat(1.0));
        // b = state - a, state = 0 initially, so b = -1
        assert!((b.extract(0) - (-1.0)).abs() < 1e-15);
    }

    #[test]
    fn test_simd_diode_newton_splatted() {
        let diode = SimdDiode::new(1e-15, 1.0, 300.0);
        // All lanes identical — result should be identical
        let a = F64x4::splat(0.1);
        let r = F64x4::splat(1000.0);
        let v = diode.solve_newton_simd(a, r);
        let v0 = v.extract(0);
        for i in 1..4 {
            assert!(
                (v.extract(i) - v0).abs() < 1e-12,
                "lane {} diverged: {} vs {}",
                i,
                v.extract(i),
                v0
            );
        }
    }

    #[test]
    fn test_simd_diode_process_batch_consistency() {
        let mut diode = SimdDiode::new(1e-15, 1.0, 300.0);
        // process_batch with all zeros
        let inputs = vec![0.0f64; 8];
        let mut outputs = vec![0.0f64; 8];
        process_batch_simd(&mut diode, &inputs, &mut outputs);
        for &o in &outputs {
            assert!(o.is_finite(), "output should be finite, got {}", o);
        }
    }

    #[test]
    fn test_simd_newton_convergence() {
        let diode = SimdDiode::new(1e-15, 1.0, 300.0);
        // Test with varied inputs — each lane should converge independently
        let a = F64x4::load(&[0.0, 0.5, 1.0, 2.0]);
        let r = F64x4::splat(1000.0);
        let v = diode.solve_newton_simd(a, r);
        for i in 0..4 {
            assert!(
                v.extract(i).is_finite(),
                "v[{}] should be finite, got {}",
                i,
                v.extract(i)
            );
        }
    }
}

/// Process a batch of samples through a SIMD WDF element
pub fn process_batch_simd(
    element: &mut dyn SimdWdfElement<SimdType = F64x4>,
    inputs: &[f64],
    outputs: &mut [f64],
) {
    let len = inputs.len().min(outputs.len());
    let chunks = len / 4;
    let remainder = len % 4;

    for i in 0..chunks {
        let offset = i * 4;
        let a = F64x4::load(&inputs[offset..offset + 4]);
        let b = element.process_incident_simd(a);
        b.store(&mut outputs[offset..offset + 4]);
    }

    if remainder > 0 {
        let offset = chunks * 4;
        let mut tail = [0.0f64; 4];
        tail[..remainder].copy_from_slice(&inputs[offset..offset + remainder]);
        let a = F64x4::load(&tail);
        let b = element.process_incident_simd(a);
        let mut b_arr = [0.0f64; 4];
        b.store(&mut b_arr);
        outputs[offset..offset + remainder].copy_from_slice(&b_arr[..remainder]);
    }
}
