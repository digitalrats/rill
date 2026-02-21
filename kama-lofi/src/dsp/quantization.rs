//! Функции для квантования и понижения битности

/// Базовое квантование с понижением битности
pub fn bitcrush(sample: f32, bit_depth: u8, dither: bool) -> f32 {
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

/// Понижение частоты дискретизации с удержанием значения
pub fn sample_rate_reduce(sample: f32, factor: usize, hold: &mut f32, counter: &mut usize) -> f32 {
    *counter += 1;
    if *counter >= factor {
        *counter = 0;
        *hold = sample;
    }
    *hold
}

/// Расчёт коэффициента понижения частоты
pub fn calculate_reduction_factor(input_sr: f32, target_sr: f32) -> usize {
    (input_sr / target_sr).ceil() as usize
}

/// Нелинейное квантование (μ-law)
pub fn nonlinear_quantize(sample: f32, bit_depth: u8) -> f32 {
    let sign = sample.signum();
    let abs_sample = sample.abs().min(1.0);
    
    let mu = 100.0;
    let compressed = sign * (1.0 + mu * abs_sample).ln() / (1.0 + mu).ln();
    
    let quantized = bitcrush(compressed, bit_depth, false);
    
    let expanded = sign * ((1.0 + mu).powf(quantized.abs()) - 1.0) / mu;
    expanded.clamp(-1.0, 1.0)
}