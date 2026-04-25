//! Трейты для анализаторов сигнала

use rill_core::AudioNum;
use crate::algorithm::Algorithm;

/// Базовый трейт для анализаторов
pub trait Analyzer<T: AudioNum>: Algorithm<T> {
    /// Тип результата анализа
    type Output;
    
    /// Получить результат анализа
    fn result(&self) -> &Self::Output;
    
    /// Сбросить накопленные данные
    fn reset_analysis(&mut self);
}

/// Пиковый детектор (VU метр)
pub trait PeakMeter<T: AudioNum>: Analyzer<T, Output = T> {
    /// Скорость спада (0.0-1.0)
    fn decay(&self) -> f32;
    
    /// Установить скорость спада
    fn set_decay(&mut self, decay: f32);
    
    /// Текущий пик (для отображения)
    fn peak(&self) -> T {
        *self.result()
    }
}

/// Детектор огибающей
pub trait EnvelopeFollower<T: AudioNum>: Analyzer<T, Output = T> {
    /// Время атаки в секундах
    fn attack(&self) -> f32;
    
    /// Время спада в секундах
    fn release(&self) -> f32;
    
    /// Установить времена
    fn set_attack_release(&mut self, attack: f32, release: f32);
    
    /// Текущая огибающая
    fn envelope(&self) -> T {
        *self.result()
    }
}

/// Детектор частоты (для тюнеров)
pub trait FrequencyDetector<T: AudioNum>: Analyzer<T, Output = f32> {
    /// Минимальная частота детектирования
    fn min_freq(&self) -> f32;
    
    /// Максимальная частота детектирования
    fn max_freq(&self) -> f32;
    
    /// Текущая частота
    fn frequency(&self) -> f32 {
        *self.result()
    }
    
    /// Ближайшая MIDI нота
    fn closest_midi_note(&self) -> u8 {
        let freq = self.frequency();
        if freq <= 0.0 {
            return 0;
        }
        let note = 69.0 + 12.0 * (freq / 440.0).log2();
        note.round() as u8
    }
}

/// Спектроанализатор (FFT)
pub trait SpectrumAnalyzer<T: AudioNum>: Analyzer<T, Output = Vec<f32>> {
    /// Размер FFT
    fn fft_size(&self) -> usize;
    
    /// Получить спектр
    fn spectrum(&self) -> &[f32] {
        self.result()
    }
    
    /// Получить амплитуду на конкретной частоте
    fn amplitude_at(&self, freq: f32, sample_rate: f32) -> f32 {
        let bin = (freq * self.fft_size() as f32 / sample_rate) as usize;
        self.result().get(bin).copied().unwrap_or(0.0)
    }
}