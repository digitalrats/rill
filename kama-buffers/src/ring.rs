//! Кольцевой буфер с фиксированным размером

use std::sync::Arc;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::view::{BufferView, BufferViewMut};

/// Кольцевой буфер с фиксированным размером
#[derive(Clone, Debug)]
pub struct RingBuffer {
    /// Внутренние данные буфера
    pub(crate) buffer: Arc<RwLock<Vec<f32>>>,
    /// Размер буфера (всегда степень двойки)
    size: usize,
    /// Текущая позиция записи
    pub(crate) write_pos: usize,
    /// Маска для быстрого вычисления остатка (size - 1)
    mask: usize,
    /// Флаг, указывающий, что буфер хотя бы раз был полностью заполнен
    pub(crate) filled: bool,
}

impl RingBuffer {
    /// Создать новый кольцевой буфер
    pub fn new(size: usize) -> Self {
        let size = size.next_power_of_two();
        Self {
            buffer: Arc::new(RwLock::new(vec![0.0; size])),
            size,
            write_pos: 0,
            mask: size - 1,
            filled: false,
        }
    }
    
    /// Получить View для чтения
    pub fn view(&self) -> BufferView<'_> {
        BufferView::new(self)
    }
    
    /// Получить View для записи (если нужен мутабельный доступ)
    pub fn view_mut(&mut self) -> BufferViewMut<'_> {
        BufferViewMut::new(self)
    }
    
    /// Записать семплы в буфер
    pub fn write(&mut self, samples: &[f32]) {
        // Используем view_mut для записи
        let mut view = self.view_mut();
        view.write_slice(samples);
    }
    
    /// Прочитать семплы с фиксированной задержкой (упрощенный API)
    pub fn read(&self, delay_samples: usize, output: &mut [f32]) {
        let view = self.view();
        for i in 0..output.len() {
            output[i] = view.read_delayed(delay_samples, i);
        }
    }
    
    /// Прочитать с интерполяцией (упрощенный API)
    pub fn read_interpolated(&self, delay_samples: f32, output: &mut [f32]) {
        let view = self.view();
        view.read_sequence_interpolated(delay_samples, output);
    }
    
    /// Получить доступ к данным для чтения (внутреннее использование)
    pub(crate) fn read_guard(&self) -> RwLockReadGuard<'_, Vec<f32>> {
        self.buffer.read()
    }
    
    /// Получить доступ к данным для записи (внутреннее использование)
    pub(crate) fn write_guard(&mut self) -> RwLockWriteGuard<'_, Vec<f32>> {
        self.buffer.write()
    }
    
    /// Получить размер буфера
    pub fn size(&self) -> usize {
        self.size
    }
    
    /// Получить текущую позицию записи
    pub fn write_pos(&self) -> usize {
        self.write_pos
    }
    
    /// Получить маску (size - 1)
    pub fn mask(&self) -> usize {
        self.mask
    }
    
    /// Проверить, заполнен ли буфер хотя бы раз
    pub fn is_filled(&self) -> bool {
        self.filled
    }
    
    /// Получить количество записанных семплов
    pub fn len(&self) -> usize {
        if self.filled {
            self.size
        } else {
            self.write_pos
        }
    }
    
    /// Сбросить буфер
    pub fn reset(&mut self) {
        let mut view = self.view_mut();
        view.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ring_buffer_basic() {
        let mut buffer = RingBuffer::new(8);
        let test_data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        buffer.write(&test_data);
        
        // После записи 5 элементов, буфер содержит [1,2,3,4,5,0,0,0]
        // но filled = false, write_pos = 5
        
        let mut output = vec![0.0; 3];
        buffer.read(1, &mut output);
        
        // delay=1: читаем последние 3 семпла в обратном порядке: 5,4,3
        assert_eq!(output, [5.0, 4.0, 3.0]);
        
        buffer.read(2, &mut output);
        assert_eq!(output, [4.0, 3.0, 2.0]);
    }
    
    #[test]
    fn test_ring_buffer_wraparound() {
        let mut buffer = RingBuffer::new(4);
        
        // Заполняем буфер полностью
        buffer.write(&[1.0, 2.0, 3.0, 4.0]);
        // Теперь filled = true, write_pos = 0
        
        let mut output = vec![0.0; 2];
        
        buffer.read(1, &mut output);
        // delay=1: последние 2 семпла: 4,3
        assert_eq!(output, [4.0, 3.0]);
        
        buffer.read(2, &mut output);
        // delay=2: семплы с индексов 2,1: 3,2
        assert_eq!(output, [3.0, 2.0]);
        
        buffer.read(3, &mut output);
        // delay=3: семплы с индексов 1,0: 2,1
        assert_eq!(output, [2.0, 1.0]);
    }
    
    #[test]
    fn test_ring_buffer_overwrite() {
        let mut buffer = RingBuffer::new(4);
        
        // Записываем больше, чем размер буфера
        buffer.write(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        // Теперь буфер должен содержать последние 4 семпла: 3,4,5,6
        
        let mut output = vec![0.0; 4];
        buffer.read(1, &mut output);
        
        // Последние 4 семпла в обратном порядке: 6,5,4,3
        assert_eq!(output, [6.0, 5.0, 4.0, 3.0]);
    }
    
    #[test]
    fn test_ring_buffer_interpolated() {
        let mut buffer = RingBuffer::new(4);
        buffer.write(&[1.0, 2.0, 3.0, 4.0]);
        
        let mut output = vec![0.0; 2];
        buffer.read_interpolated(1.5, &mut output);
        
        // delay=1.5: между 4.0 и 3.0 с frac=0.5 -> 3.5
        // затем между 3.0 и 2.0 -> 2.5
        assert!((output[0] - 3.5).abs() < 1e-6);
        assert!((output[1] - 2.5).abs() < 1e-6);
    }
    
    #[test]
    fn test_ring_buffer_interpolated_wraparound() {
        let mut buffer = RingBuffer::new(4);
        buffer.write(&[1.0, 2.0, 3.0, 4.0]);
        // write_pos = 0, filled = true
        
        println!("write_pos = {}, size = {}", buffer.write_pos, buffer.size);
        println!("buffer contents: {:?}", &*buffer.buffer.read());
        
        let mut output = vec![0.0; 2];
        buffer.read_interpolated(0.5, &mut output);
        
        println!("output: {:?}", output);
        
        // delay=0.5: между 4.0 (индекс 3) и 1.0 (индекс 0) с frac=0.5 -> 2.5
        // затем между 1.0 (индекс 0) и 2.0 (индекс 1) -> 1.5
        assert!((output[0] - 2.5).abs() < 1e-6, "output[0] = {}, expected 2.5", output[0]);
        assert!((output[1] - 1.5).abs() < 1e-6, "output[1] = {}, expected 1.5", output[1]);
    }
    
    #[test]
    fn test_ring_buffer_len() {
        let mut buffer = RingBuffer::new(4);
        assert_eq!(buffer.len(), 0);
        
        buffer.write(&[1.0, 2.0]);
        assert_eq!(buffer.len(), 2);
        
        buffer.write(&[3.0, 4.0]);
        assert_eq!(buffer.len(), 4);
        assert!(buffer.is_filled());
        
        buffer.write(&[5.0, 6.0]);
        assert_eq!(buffer.len(), 4); // Размер не меняется после перезаписи
        assert!(buffer.is_filled());
    }
    
    #[test]
    fn test_ring_buffer_reset() {
        let mut buffer = RingBuffer::new(4);
        buffer.write(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(buffer.len(), 4);
        
        buffer.reset();
        assert_eq!(buffer.len(), 0);
        assert!(!buffer.is_filled());
        
        let view = buffer.view();
        for i in 0..4 {
            assert_eq!(view.get(i), 0.0);
        }
    }
}