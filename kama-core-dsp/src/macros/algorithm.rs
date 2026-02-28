//! Трейты для DSP алгоритмов (не узлов!)

use kama_core::traits::ParamValue;

/// Базовый трейт для всех DSP алгоритмов
///
/// Алгоритм - это чистая DSP логика, не зависящая от графа и параметров.
/// Он оперирует только числами и внутренним состоянием.
pub trait Algorithm<T>: Send + Sync {
    /// Инициализация алгоритма
    fn init(&mut self, sample_rate: f32);
    
    /// Сброс внутреннего состояния
    fn reset(&mut self);
    
    /// Обработка одного семпла (моно)
    fn process(&mut self, input: T) -> T;
    
    /// Обработка блока семплов (можно переопределить для оптимизации)
    fn process_block(&mut self, input: &[T], output: &mut [T]) {
        let len = input.len().min(output.len());
        for i in 0..len {
            output[i] = self.process(input[i]);
        }
    }
    
    /// Имя алгоритма
    fn name(&self) -> &'static str;
}

/// Фильтр - алгоритм с частотной характеристикой
pub trait Filter<T>: Algorithm<T> {
    /// Установить частоту среза
    fn set_cutoff(&mut self, freq: f32);
    
    /// Установить добротность
    fn set_q(&mut self, q: f32);
    
    /// Установить усиление (для peak/shelving)
    fn set_gain_db(&mut self, gain: f32);
}

/// Эффект - алгоритм с возможностью dry/wet
pub trait Effect<T>: Algorithm<T> {
    /// Установить соотношение dry/wet
    fn set_dry_wet(&mut self, mix: f32);
    
    /// Получить сухой сигнал
    fn dry(&self) -> T;
    
    /// Получить обработанный сигнал
    fn wet(&self) -> T;
}

/// Генератор - источник сигнала
pub trait Generator<T>: Algorithm<T> {
    /// Текущая фаза (0..1)
    fn phase(&self) -> T;
    
    /// Сбросить фазу
    fn reset_phase(&mut self);
    
    /// Установить частоту
    fn set_frequency(&mut self, freq: f32);
    
    /// Установить амплитуду
    fn set_amplitude(&mut self, amp: T);
}