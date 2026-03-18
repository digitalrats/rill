//! Контекст выполнения DSP-алгоритмов

use kama_core::AudioNum;

/// Контекст DSP-обработки
///
/// Предоставляет информацию о текущем состоянии обработки:
/// - временные метки
/// - частота дискретизации
/// - размер блока
/// - и т.д.
#[derive(Debug, Clone)]
pub struct DspContext<T: AudioNum> {
    
    /// Текущая частота дискретизации
    pub sample_rate: f32,
    
    /// Размер текущего обрабатываемого блока
    pub block_size: usize,
    
    /// Абсолютная позиция текущего блока (в семплах)
    pub block_position: usize,
    
    /// Тип данных для текущей обработки
    pub _phantom: std::marker::PhantomData<T>,
}

impl<'a, T: AudioNum> DspContext<T> {
    /// Создать новый контекст
    pub fn new(
        sample_rate: f32,
        block_size: usize,
        block_position: usize,
    ) -> Self {
        Self {
        sample_rate,
            block_size,
            block_position,
            _phantom: std::marker::PhantomData,
        }
    }
    
    /// Получить текущую позицию в секундах
    pub fn seconds(&self) -> f64 {
        self.block_position as f64 / self.sample_rate as f64
    }
}