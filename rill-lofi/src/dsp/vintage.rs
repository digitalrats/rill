//! Винтажные эффекты

use super::*;
use std::f32::consts::PI;

/// Эмуляция ленточной сатурации
pub fn tape_saturation(sample: f32, drive: f32) -> f32 {
    let driven = sample * drive;
    // Мягкое ограничение как у ленты
    (driven / (1.0 + driven.abs())).clamp(-1.0, 1.0)
}

/// Эмуляция винилового шума и треска
pub fn vinyl_noise(sample: f32, _time: f32, _sample_rate: f32) -> f32 {
    let noise_level = 0.02;
    let crackle_prob = 0.001;
    
    let noise = crate::dsp::noise::white_noise(noise_level);
    let crackle = crate::dsp::noise::crackle(crackle_prob, 0.1);
    
    sample + noise + crackle
}

/// Эмуляция Wow и Flutter (нестабильность скорости)
pub fn wow_flutter(sample: f32, time: f32, depth: f32, rate: f32) -> f32 {
    let modulation = (2.0 * PI * rate * time).sin() * depth;
    sample * (1.0 + modulation)
}