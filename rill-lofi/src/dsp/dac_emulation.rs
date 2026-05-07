//! Digital-to-analog converter emulation

use std::f32::consts::PI;

/// DAC model with nonlinearities
pub fn nonlinear_dac(sample: f32, nonlinearity: f32) -> f32 {
    sample * (1.0 + nonlinearity * sample.abs())
}

/// Resistor-ladder DAC emulation (like NES)
pub fn r2r_dac(sample: f32) -> f32 {
    let steps = 256.0;
    let stepped = (sample * steps).round() / steps;
    // Nonlinearity from resistor tolerances
    stepped * (1.0 + 0.05 * (2.0 * PI * stepped).sin())
}

/// Logarithmic DAC emulation (like Akai S900)
pub fn logarithmic_dac(sample: f32) -> f32 {
    sample.signum() * (1.0 - (-sample.abs() * 5.0).exp())
}

/// Select DAC model by system
pub fn for_system(system: crate::config::ClassicSystem, sample: f32) -> f32 {
    match system {
        crate::config::ClassicSystem::Nes => r2r_dac(sample),
        crate::config::ClassicSystem::AkaiS900 => logarithmic_dac(sample),
        _ => nonlinear_dac(sample, 0.1),
    }
}
