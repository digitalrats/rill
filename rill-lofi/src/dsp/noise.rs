//! Generation of various noise types

use rand::Rng;

/// White noise with given level
pub fn white_noise(level: f32) -> f32 {
    let mut rng = rand::thread_rng();
    (rng.gen::<f32>() - 0.5) * 2.0 * level
}

/// Pink noise (simple approximation)
pub fn pink_noise(level: f32, _sample_rate: f32) -> f32 {
    static mut LAST_NOISE: f32 = 0.0;
    static mut FILTER_STATE: [f32; 3] = [0.0; 3];

    let white = white_noise(level);

    unsafe {
        // Simple low-pass filter for coloration
        let cutoff = 1000.0 / 44100.0;
        FILTER_STATE[0] = FILTER_STATE[0] + cutoff * (white - FILTER_STATE[0]);
        FILTER_STATE[1] = FILTER_STATE[1] + cutoff * (FILTER_STATE[0] - FILTER_STATE[1]);
        FILTER_STATE[2] = FILTER_STATE[2] + cutoff * (FILTER_STATE[1] - FILTER_STATE[2]);

        LAST_NOISE = FILTER_STATE[2];
        LAST_NOISE
    }
}

/// Vinyl/tape crackle (random impulses)
pub fn crackle(probability: f32, level: f32) -> f32 {
    let mut rng = rand::thread_rng();
    if rng.gen::<f32>() < probability {
        (rng.gen::<f32>() - 0.5) * 2.0 * level
    } else {
        0.0
    }
}

/// System-specific noise
pub fn system_noise(system: crate::config::ClassicSystem, sample: f32) -> f32 {
    let noise_level = match system {
        crate::config::ClassicSystem::Nes => 0.05,
        crate::config::ClassicSystem::Commodore64 => 0.03,
        crate::config::ClassicSystem::AkaiS900 => 0.01,
        crate::config::ClassicSystem::FairlightCMI => 0.04,
        crate::config::ClassicSystem::Custom { noise_floor, .. } => noise_floor / 100.0,
        _ => 0.02,
    };

    sample + white_noise(noise_level)
}
