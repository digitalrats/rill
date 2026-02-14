use rand::Rng;
use std::f32::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DacModel {
    Ideal,
    R2R,
    PWM,
    Multibit,
}

pub fn quantize(sample: f32, bit_depth: u8, dither: bool) -> f32 {
    if bit_depth >= 24 {
        return sample;
    }
    
    let steps = (1u32 << bit_depth) as f32;
    let max_val = 1.0 - (1.0 / steps);
    
    let scaled = sample.clamp(-1.0, 1.0) * max_val;
    
    if dither {
        let dither_amount = 1.0 / steps;
        let dither_sample = (rand::random::<f32>() - 0.5) * 2.0 * dither_amount;
        ((scaled + dither_sample) * steps).round() / steps
    } else {
        (scaled * steps).round() / steps
    }
}

pub fn nonlinear_quantize(sample: f32, bit_depth: u8) -> f32 {
    let sign = sample.signum();
    let abs_sample = sample.abs().min(1.0);
    
    let mu = 100.0;
    let compressed = sign * (1.0 + mu * abs_sample).ln() / (1.0 + mu).ln();
    
    let _quantized = quantize(compressed, bit_depth, false);
    
    let expanded = sign * ((1.0 + mu).ln().exp() - 1.0) / mu;
    expanded.clamp(-1.0, 1.0)
}

pub fn reduce_sample_rate(input: &[f32], output: &mut [f32], factor: usize) {
    if factor <= 1 {
        output.copy_from_slice(input);
        return;
    }
    
    for (i, out) in output.iter_mut().enumerate() {
        let src_idx = i * factor;
        if src_idx < input.len() {
            *out = input[src_idx];
        } else {
            *out = 0.0;
        }
    }
}

pub fn dac_nonlinearity(sample: f32, model: DacModel) -> f32 {
    match model {
        DacModel::Ideal => sample,
        DacModel::R2R => {
            let steps = 256.0;
            let stepped = (sample * steps).round() / steps;
            let nonlinear = stepped * (1.0 + 0.05 * (2.0 * PI * stepped).sin());
            nonlinear.clamp(-1.0, 1.0)
        }
        DacModel::PWM => {
            let pwm_noise = (rand::random::<f32>() - 0.5) * 0.01;
            (sample + pwm_noise).clamp(-1.0, 1.0)
        }
        DacModel::Multibit => {
            let mismatch = 0.02 * (sample * 3.0).sin();
            (sample + mismatch).clamp(-1.0, 1.0)
        }
    }
}

pub fn add_thermal_noise(sample: f32, amount: f32) -> f32 {
    let noise = (rand::random::<f32>() - 0.5) * 2.0 * amount;
    (sample + noise).clamp(-1.0, 1.0)
}

pub fn apply_clock_drift(sample_rate: f32, drift: f32, time: f32) -> f32 {
    let drift_variation = 1.0 + drift * 0.01 * (2.0 * PI * 0.1 * time).sin();
    sample_rate * drift_variation
}

pub fn voltage_sag(sample: f32, sag: f32) -> f32 {
    let sag_factor = 1.0 - sag;
    sample * sag_factor
}

pub fn process_lofi_chain(
    input: f32,
    bit_depth: u8,
    _sample_rate_factor: f32,
    hardware: &crate::config::HardwareEmulation,
    _time: f32,
) -> f32 {
    let mut sample = input;
    
    sample = voltage_sag(sample, hardware.voltage_drop);
    
    sample = if hardware.dac_nonlinearity {
        nonlinear_quantize(sample, bit_depth)
    } else {
        quantize(sample, bit_depth, true)
    };
    
    sample = dac_nonlinearity(sample, DacModel::R2R);
    sample = add_thermal_noise(sample, hardware.thermal_noise);
    sample = sample * (1.0 - hardware.ageing_effect * 0.5);
    
    sample.clamp(-1.0, 1.0)
}