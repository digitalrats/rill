use crate::dsp;
use std::f32::consts::PI;

pub fn create_8bit_sound(samples: &[f32], bit_depth: u8) -> Vec<f32> {
    samples.iter()
        .map(|&s| dsp::quantize(s, bit_depth, true))
        .collect()
}

pub fn add_vintage_noise(samples: &[f32], noise_level: f32) -> Vec<f32> {
    samples.iter()
        .map(|&s| dsp::add_thermal_noise(s, noise_level))
        .collect()
}

pub fn add_tape_degradation(samples: &[f32], wear: f32) -> Vec<f32> {
    let mut result = Vec::with_capacity(samples.len());
    let mut high_freq_loss = 1.0 - wear * 0.5;
    
    for (i, &sample) in samples.iter().enumerate() {
        let filtered = sample * high_freq_loss;
        let wow_flutter = 0.001 * wear * (2.0 * PI * i as f32 * 0.5 / 44100.0).sin();
        let pitched = filtered * (1.0 + wow_flutter);
        
        let dropout_chance = wear * 0.001;
        let final_sample = if rand::random::<f32>() < dropout_chance {
            0.0
        } else {
            pitched
        };
        
        result.push(final_sample.clamp(-1.0, 1.0));
        high_freq_loss *= 0.99999;
    }
    
    result
}

pub fn create_radio_effect(samples: &[f32], sample_rate: f32) -> Vec<f32> {
    let mut result = samples.to_vec();
    let center_freq = 1000.0;
    let q = 2.0;
    
    for i in 2..result.len() {
        let alpha = (PI * center_freq / sample_rate).sin() / (2.0 * q);
        let a0 = 1.0 + alpha;
        
        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a1 = -2.0 * (2.0 * PI * center_freq / sample_rate).cos();
        let a2 = 1.0 - alpha;
        
        result[i] = (b0 * samples[i] + b1 * samples[i-1] + b2 * samples[i-2]
                    - a1 * result[i-1] - a2 * result[i-2]) / a0;
    }
    
    for sample in result.iter_mut() {
        let am_noise = 0.05 * (2.0 * PI * 50.0 * rand::random::<f32>()).sin();
        *sample = (*sample * (1.0 + am_noise)).clamp(-1.0, 1.0);
    }
    
    result
}