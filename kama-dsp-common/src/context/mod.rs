//! # Контекст выполнения DSP-узлов
//! 
//! Предоставляет [`DspContext`] — структуру, содержащую всю необходимую информацию
//! для работы DSP-узла в реальном времени:
//! 
//! - **Время** — текущая позиция в семплах, секундах и тактах
//! - **Частота дискретизации** — для расчёта коэффициентов
//! - **Размер блока** — количество обрабатываемых семплов за вызов
//! - **Менеджер буферов** — для временных выделений памяти
//! - **Пользовательские данные** — для расширения функциональности

//! Контекст DSP-обработки

use kama_buffers::BufferManager;

/// Контекст выполнения DSP-узла
#[derive(Clone)]  // Убираем Debug из derive
pub struct DspContext<'a> {
    /// Провайдер времени
    /// Провайдер времени (позиция, BPM, такты).
    pub time: &'a dyn kama_core_traits::TimeProvider,
    
    /// Текущая частота дискретизации
    /// Текущая частота дискретизации в Hz.
    pub sample_rate: f32,
    
    /// Размер текущего обрабатываемого блока
    /// Размер текущего обрабатываемого блока (в семплах).
    pub block_size: usize,
    
    /// Позиция текущего блока
    /// Абсолютная позиция текущего блока (в семплах).
    pub block_position: usize,
    
    /// Менеджер буферов
    /// Менеджер буферов для временных выделений.
    pub buffers: &'a BufferManager,
    
    /// Дополнительные пользовательские данные
    /// Опциональные пользовательские данные.
    pub user_data: Option<&'a dyn std::any::Any>,
}

// Ручная реализация Debug
impl<'a> std::fmt::Debug for DspContext<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DspContext")
            .field("sample_rate", &self.sample_rate)
            .field("block_size", &self.block_size)
            .field("block_position", &self.block_position)
            .field("buffers", self.buffers)
            .field("user_data", &self.user_data.is_some())
            .finish()
    }
}

impl<'a> DspContext<'a> {
    /// Создать новый контекст
    /// Создать новый контекст.
    pub fn new(
        time: &'a dyn kama_core_traits::TimeProvider,
        sample_rate: f32,
        block_size: usize,
        block_position: usize,
        buffers: &'a BufferManager,
    ) -> Self {
        Self {
            time,
            sample_rate,
            block_size,
            block_position,
            buffers,
            user_data: None,
        }
    }
    
    /// Добавить пользовательские данные
    pub fn with_user_data<T: 'static>(mut self, data: &'a T) -> Self {
        self.user_data = Some(data);
        self
    }
    
    /// Получить текущую позицию в секундах
    /// Получить текущую позицию в секундах.
    pub fn seconds(&self) -> f64 {
        self.block_position as f64 / self.sample_rate as f64
    }
    
    /// Получить текущую позицию в тактах (упрощенно)
    /// Получить текущую позицию в тактах (упрощённо, исходя из BPM).
    pub fn beats(&self) -> f64 {
        let bpm = self.time.bpm();
        let seconds_per_beat = 60.0 / bpm;
        self.seconds() / seconds_per_beat
    }
}