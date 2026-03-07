//! Трейты для генераторов (источников сигнала)

use kama_core::AudioNum;
use crate::algorithm::Algorithm;

/// Базовый трейт для генераторов
pub trait Generator<T: AudioNum>: Algorithm<T> {
    /// Генерировать следующий семпл
    fn generate(&mut self) -> T {
        self.process_sample(T::ZERO)
    }
    
    /// Генерировать блок семплов (по умолчанию использует generate)
    fn generate_block(&mut self, output: &mut [T]) {
        for out in output.iter_mut() {
            *out = self.generate();
        }
    }
    
    /// Генерировать блок с использованием векторного eDSL (опционально)
    fn generate_block_vector(&mut self, output: &mut [T]) {
        self.generate_block(output);
    }
    
    /// Сбросить фазу
    fn reset_phase(&mut self);
    
    /// Текущая фаза (0.0 - 1.0)
    fn phase(&self) -> f32;
}

/// Генератор с изменяемой частотой
pub trait FrequencySource<T: AudioNum>: Generator<T> {
    /// Установить частоту в Hz
    fn set_frequency(&mut self, freq: f32);
    
    /// Получить текущую частоту
    fn frequency(&self) -> f32;
    
    /// Установить частоту в MIDI нотах
    fn set_midi_note(&mut self, note: u8) {
        let freq = 440.0 * 2.0_f32.powf((note as f32 - 69.0) / 12.0);
        self.set_frequency(freq);
    }
}

/// Генератор с изменяемой амплитудой
pub trait AmplitudeSource<T: AudioNum>: Generator<T> {
    /// Установить амплитуду (0.0-1.0)
    fn set_amplitude(&mut self, amp: T);
    
    /// Получить текущую амплитуду
    fn amplitude(&self) -> T;
    
    /// Установить амплитуду в dB
    fn set_amplitude_db(&mut self, db: f32) {
        self.set_amplitude(T::from_f32(10.0_f32.powf(db / 20.0)));
    }
}

/// Генератор с изменяемой формой волны
pub trait WaveformSource<T: AudioNum>: Generator<T> {
    /// Тип波形
    type Waveform;
    
    /// Установить форму волны
    fn set_waveform(&mut self, wf: Self::Waveform);
    
    /// Получить текущую форму волны
    fn waveform(&self) -> Self::Waveform;
}

/// Шумовой генератор
pub trait NoiseSource<T: AudioNum>: Generator<T> {
    /// Тип шума
    type NoiseType;
    
    /// Установить тип шума
    fn set_noise_type(&mut self, nt: Self::NoiseType);
    
    /// Получить тип шума
    fn noise_type(&self) -> Self::NoiseType;
}