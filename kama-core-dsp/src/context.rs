//! Контекст выполнения DSP-алгоритмов

use kama_core::AudioNum;
use kama_core::time::TimeProvider;

/// Контекст DSP-обработки
///
/// Предоставляет информацию о текущем состоянии обработки:
/// - временные метки
/// - частота дискретизации
/// - размер блока
/// - и т.д.
#[derive(Debug, Clone)]
pub struct DspContext<'a, T: AudioNum> {
    /// Провайдер времени
    pub time: &'a dyn TimeProvider,
    
    /// Текущая частота дискретизации
    pub sample_rate: f32,
    
    /// Размер текущего обрабатываемого блока
    pub block_size: usize,
    
    /// Абсолютная позиция текущего блока (в семплах)
    pub block_position: usize,
    
    /// Тип данных для текущей обработки
    pub _phantom: std::marker::PhantomData<T>,
}

impl<'a, T: AudioNum> DspContext<'a, T> {
    /// Создать новый контекст
    pub fn new(
        time: &'a dyn TimeProvider,
        sample_rate: f32,
        block_size: usize,
        block_position: usize,
    ) -> Self {
        Self {
            time,
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
    
    /// Получить текущую позицию в тактах
    pub fn beats(&self) -> f64 {
        let bpm = self.time.bpm();
        let seconds_per_beat = 60.0 / bpm;
        self.seconds() / seconds_per_beat
    }
    
    /// Получить информацию о текущем такте
    pub fn tick_info(&self) -> kama_core::time::TickInfo {
        self.time.tick_info()
    }
    
    /// Проверить, является ли текущий блок началом такта
    pub fn is_new_bar(&self) -> bool {
        let info = self.tick_info();
        info.beat == 0 && info.sixteenth == 0
    }
    
    /// Проверить, является ли текущий блок началом доли
    pub fn is_new_beat(&self) -> bool {
        let info = self.tick_info();
        info.sixteenth == 0
    }
}