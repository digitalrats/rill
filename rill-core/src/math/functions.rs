//! Общие математические функции для аудиообработки

use super::num::AudioNum;

/// Линейная интерполяция
#[inline(always)]
pub fn lerp<T: AudioNum>(a: T, b: T, t: T) -> T {
    a + (b - a) * t
}

/// Преобразовать децибелы в линейный коэффициент
#[inline(always)]
pub fn db_to_linear<T: AudioNum>(db: T) -> T {
    T::from_f32(10.0_f32.powf(db.to_f32() / 20.0))
}

/// Преобразовать линейный коэффициент в децибелы
#[inline(always)]
pub fn linear_to_db<T: AudioNum>(linear: T) -> T {
    T::from_f32(20.0 * linear.to_f32().log10())
}

/// Преобразовать MIDI ноту в частоту
#[inline(always)]
pub fn midi_to_freq<T: AudioNum>(note: u8) -> T {
    let exp = (note as f32 - 69.0) / 12.0;
    T::from_f32(440.0 * 2.0_f32.powf(exp))
}

/// Преобразовать частоту в MIDI ноту
#[inline(always)]
pub fn freq_to_midi<T: AudioNum>(freq: T) -> f32 {
    69.0 + 12.0 * (freq.to_f32() / 440.0).log2()
}

/// Преобразовать секунды в семплы
#[inline(always)]
pub fn seconds_to_samples(seconds: f32, sample_rate: f32) -> usize {
    (seconds * sample_rate) as usize
}

/// Преобразовать семплы в секунды
#[inline(always)]
pub fn samples_to_seconds(samples: usize, sample_rate: f32) -> f32 {
    samples as f32 / sample_rate
}

/// Быстрая аппроксимация tanh
#[inline(always)]
pub fn fast_tanh<T: AudioNum>(x: T) -> T {
    let xf = x.to_f32();
    T::from_f32(xf / (1.0 + xf.abs()))
}

/// Мягкое клиппирование
#[inline(always)]
pub fn soft_clip<T: AudioNum>(x: T, threshold: T) -> T {
    let xf = x.to_f32();
    let t = threshold.to_f32();

    if xf > t {
        T::from_f32(t + (xf - t) / (1.0 + ((xf - t) / (1.0 - t)).powi(2)))
    } else if xf < -t {
        T::from_f32(-t - (-xf - t) / (1.0 + ((-xf - t) / (1.0 - t)).powi(2)))
    } else {
        x
    }
}

/// Окно Ханна
#[inline(always)]
pub fn hann_window<T: AudioNum>(x: T) -> T {
    let cos_term = (x * T::from_f32(2.0) * T::PI).cos();
    T::from_f32(0.5) * (T::ONE - cos_term)
}
