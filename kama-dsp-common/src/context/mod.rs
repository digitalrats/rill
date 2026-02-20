//! Контекст DSP-обработки

use kama_buffers::BufferRegistry;

/// Контекст выполнения DSP-узла
///
/// Предоставляет узлу всю необходимую информацию о текущем состоянии обработки:
/// - Время (позиция, BPM, такты)
/// - Параметры (доступ к значениям параметров)
/// - Буферы (доступ к внешним буферам по имени)
/// - Информацию о блоке (размер, позиция)
#[derive(Debug, Clone)]
pub struct DspContext<'a> {
    /// Провайдер времени (позиция в сэмплах/секундах, BPM, такты)
    pub time: &'a dyn kama_core_traits::TimeProvider,
    
    /// Текущая частота дискретизации
    pub sample_rate: f32,
    
    /// Размер текущего обрабатываемого блока (в сэмплах)
    pub block_size: usize,
    
    /// Позиция текущего блока относительно начала потока (в сэмплах)
    pub block_position: usize,
    
    /// Реестр для доступа к внешним буферам по имени
    pub buffers: &'a BufferRegistry,
    
    /// Дополнительные пользовательские данные (опционально)
    pub user_data: Option<&'a dyn std::any::Any>,
}

impl<'a> DspContext<'a> {
    /// Создать новый контекст
    pub fn new(
        time: &'a dyn kama_core_traits::TimeProvider,
        sample_rate: f32,
        block_size: usize,
        block_position: usize,
        buffers: &'a BufferRegistry,
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
    pub fn seconds(&self) -> f64 {
        self.block_position as f64 / self.sample_rate as f64
    }
    
    /// Получить текущую позицию в тактах (упрощенно)
    pub fn beats(&self) -> f64 {
        let bpm = self.time.bpm();
        let seconds_per_beat = 60.0 / bpm;
        self.seconds() / seconds_per_beat
    }
}