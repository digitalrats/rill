//! Гребенчатый фильтр (Comb Filter)
//!
//! Используется в:
//! - Реверберации (серии гребенчатых фильтров)
//! - Эффектах "металлического" звука
//! - Физическом моделировании струн
use kama_core::math::AudioNum;
use kama_core::buffer::DelayLine;
use crate::algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};
use super::{FilterParams, FilterType};

/// Гребенчатый фильтр
pub struct CombFilter<T: AudioNum, const MAX_DELAY: usize> {
    params: FilterParams,
    delay: DelayLine<T, MAX_DELAY>,
    feedback: T,
    delay_samples: usize,
    sample_rate: f32,
}

impl<T: AudioNum, const MAX_DELAY: usize> CombFilter<T, MAX_DELAY> {
    /// Создать новый гребенчатый фильтр
    pub fn new(params: FilterParams, feedback: f32) -> Self {
        Self {
            params,
            delay: DelayLine::new(44100.0),
            feedback: T::from_f32(feedback),
            delay_samples: 0,
            sample_rate: 44100.0,
        }
    }
    
    /// Обновить задержку на основе частоты среза
    fn update_delay(&mut self) {
        // Для гребенчатого фильтра задержка = sample_rate / cutoff
        self.delay_samples = (self.sample_rate / self.params.cutoff) as usize;
        self.delay.set_delay_samples(self.delay_samples);
    }
    
    /// Установить обратную связь
    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = T::from_f32(feedback);
    }
}

impl<T: AudioNum, const MAX_DELAY: usize> Algorithm<T> for CombFilter<T, MAX_DELAY> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_delay();
        self.delay.clear();
    }
    
    fn reset(&mut self) {
        self.delay.clear();
    }
    
    fn process_sample(&mut self, input: T) -> T {
        // Читаем задержанный сигнал
        // Для задержки используем self.delay_samples
        let delayed = self.delay.read_delayed(self.delay_samples);
        
        // Выход - задержанный сигнал
        let output = delayed;
        
        // Записываем с обратной связью
        let write_signal = input + delayed * self.feedback;
        let _ = self.delay.write(write_signal);
        
        output
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Comb Filter",
            category: AlgorithmCategory::Filter,
            description: "Comb filter for reverb and physical modeling",
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: AudioNum, const MAX_DELAY: usize> ParameterizedAlgorithm<T> for CombFilter<T, MAX_DELAY> {
    type Params = FilterParams;
    
    fn params(&self) -> &Self::Params {
        &self.params
    }
    
    fn set_params(&mut self, params: Self::Params) {
        self.params = params;
        self.update_delay();
    }
}

// Blanket implementation в mod.rs возьмёт на себя Filter