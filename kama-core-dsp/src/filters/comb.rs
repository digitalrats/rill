//! Гребенчатый фильтр (Comb Filter)
//!
//! Используется в:
//! - Реверберации (серии гребенчатых фильтров)
//! - Эффектах "металлического" звука
//! - Физическом моделировании струн

use crate::math::AudioNum;
use crate::buffer::DelayLine;
use super::{Filter, FilterParams};
use crate::algorithm::{Algorithm, ParameterizedAlgorithm, AlgorithmMetadata, AlgorithmCategory};

/// Гребенчатый фильтр
pub struct CombFilter<T: AudioNum, const MAX_DELAY: usize> {
    /// Параметры фильтра
    params: FilterParams,
    /// Линия задержки
    delay: DelayLine<T, MAX_DELAY>,
    /// Коэффициент обратной связи
    feedback: T,
    /// Задержка в семплах
    delay_samples: usize,
    /// Частота дискретизации
    sample_rate: f32,
}

impl<T: AudioNum, const MAX_DELAY: usize> CombFilter<T, MAX_DELAY> {
    /// Создать новый гребенчатый фильтр
    pub fn new(params: FilterParams, feedback: f32) -> Self {
        Self {
            params,
            delay: DelayLine::new(),
            feedback: T::from_f32(feedback),
            delay_samples: 0,
            sample_rate: 44100.0,
        }
    }
    
    /// Обновить задержку
    fn update_delay(&mut self) {
        self.delay_samples = (self.params.cutoff / self.sample_rate) as usize;
        // Для comb фильтра cutoff интерпретируется как частота,
        // обратная задержке: delay = 1/freq
        self.delay_samples = (self.sample_rate / self.params.cutoff) as usize;
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
        self.delay.reset();
    }
    
    fn reset(&mut self) {
        self.delay.reset();
    }
    
    fn process_sample(&mut self, input: T) -> T {
        // Читаем задержанный сигнал
        let delayed = self.delay.read();
        
        // Вычисляем выход
        let output = delayed;
        
        // Записываем с обратной связью
        let write_signal = input.add(delayed.mul(self.feedback));
        let _ = self.delay.write(write_signal);
        
        output
    }
    
    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Comb Filter",
            category: AlgorithmCategory::Filter,
            description: "Comb filter for reverb and physical modeling".to_string(),
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