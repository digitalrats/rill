//! Базовые трейты для DSP алгоритмов
//!
//! Алгоритм - это чистая DSP логика, не зависящая от графа.
//! Все алгоритмы должны быть RT-safe.

use kama_core::traits::ParamValue;
use kama_core::AudioNum;
use std::any::Any;

/// Метаданные алгоритма
#[derive(Debug, Clone)]
pub struct AlgorithmMetadata {
    /// Название алгоритма
    pub name: &'static str,
    /// Категория
    pub category: AlgorithmCategory,
    /// Описание
    pub description: &'static str,
    /// Автор
    pub author: &'static str,
    /// Версия
    pub version: &'static str,
}

/// Категория алгоритма
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AlgorithmCategory {
    Generator,
    Filter,
    Effect,
    Analyzer,
    Utility,
}

/// Базовый трейт для всех DSP алгоритмов
///
/// Алгоритмы работают только с блоками семплов, используя векторные операции.
pub trait Algorithm<T: AudioNum>: Send + Sync {
    /// Инициализация алгоритма
    fn init(&mut self, sample_rate: f32) {
        // По умолчанию ничего не делаем
    }

    /// Сброс внутреннего состояния
    fn reset(&mut self);

    /// Обработка блока семплов с использованием векторных операций
    ///
    /// Алгоритмы должны использовать векторный eDSL через `vec_map!` или другие
    /// векторные примитивы для оптимальной производительности.
    fn process_block(&mut self, input: &[T], output: &mut [T]);

    /// Получить метаданные алгоритма
    fn metadata(&self) -> AlgorithmMetadata;

    /// Для downcasting (опционально)
    fn as_any(&self) -> &dyn std::any::Any
    where
        Self: 'static + Sized, // Добавляем Sized
    {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any
    where
        Self: 'static + Sized, // Добавляем Sized
    {
        self
    }
}

/// Алгоритм с параметрами
pub trait ParameterizedAlgorithm<T: AudioNum>: Algorithm<T> {
    /// Тип параметров
    type Params: Clone + Send + Sync;

    /// Получить текущие параметры
    fn params(&self) -> &Self::Params;

    /// Установить новые параметры
    fn set_params(&mut self, params: Self::Params);

    /// Обновить параметр по имени (для автоматизации)
    fn set_parameter(&mut self, name: &str, value: ParamValue) -> Result<(), &'static str> {
        Err(format!("Parameter '{}' not supported", name).leak())
    }
}

/// Алгоритм с контролем качества (для тестирования)
pub trait QualityMetrics<T: AudioNum>: Algorithm<T> {
    /// Вычислить SNR (Signal-to-Noise Ratio)
    fn snr(&self, reference: &[T], output: &[T]) -> f64 {
        // Базовая реализация
        0.0
    }

    /// Вычислить THD (Total Harmonic Distortion)
    fn thd(&self, frequency: f32, amplitude: T) -> f64 {
        0.0
    }
}
