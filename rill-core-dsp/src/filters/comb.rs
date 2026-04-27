//! Гребенчатый фильтр (Comb Filter)
//!
//! Используется в:
//! - Реверберации (серии гребенчатых фильтров)
//! - Эффектах "металлического" звука
//! - Физическом моделировании струн
use super::{FilterParams, FilterType};
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata, ParameterizedAlgorithm};
use crate::vector::{ScalarVector1, Vector};
use rill_core::buffer::DelayLine;
use rill_core::math::AudioNum;
use rill_core::traits::{ActionContext, ProcessResult};

/// Гребенчатый фильтр
pub struct CombFilter<T: AudioNum, const MAX_DELAY: usize> {
    params: FilterParams,
    delay: DelayLine<T, MAX_DELAY>,
    feedback: ScalarVector1<T>,
    delay_samples: usize,
    sample_rate: f32,
}

impl<T: AudioNum, const MAX_DELAY: usize> CombFilter<T, MAX_DELAY> {
    /// Создать новый гребенчатый фильтр
    pub fn new(params: FilterParams, feedback: f32) -> Self {
        Self {
            params,
            delay: DelayLine::new(44100.0),
            feedback: ScalarVector1::splat(T::from_f32(feedback)),
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
        self.feedback = ScalarVector1::splat(T::from_f32(feedback));
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

    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        for i in 0..len {
            // Читаем задержанный сигнал
            let delayed = self.delay.read_delayed(self.delay_samples);

            // Выход - задержанный сигнал
            output[i] = delayed;

            // Записываем с обратной связью
            let write_signal = input[i] + delayed * self.feedback.extract(0);
            let _ = self.delay.write(write_signal);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Comb Filter",
            category: AlgorithmCategory::Filter,
            description: "Comb filter for reverb and physical modeling",
            author: "Rill",
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
