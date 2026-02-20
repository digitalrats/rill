//! Представления буферов для безопасного доступа

use parking_lot::{RwLockReadGuard, RwLockWriteGuard};
use crate::RingBuffer;

/// Представление буфера для чтения
pub struct BufferView<'a> {
    data: RwLockReadGuard<'a, Vec<f32>>,
    size: usize,
    write_pos: usize,
    mask: usize,
    filled: bool,
}

impl<'a> BufferView<'a> {
    /// Создать новое представление из RingBuffer
    pub fn new(buffer: &'a RingBuffer) -> Self {
        // Получаем все необходимые данные ДО создания guard
        let size = buffer.size();
        let write_pos = buffer.write_pos();
        let mask = buffer.mask();
        let filled = buffer.is_filled();
        
        Self {
            data: buffer.read_guard(),
            size,
            write_pos,
            mask,
            filled,
        }
    }
    
    /// Получить семпл по индексу
    pub fn get(&self, index: usize) -> f32 {
        self.data[index % self.size]
    }
    
    /// Получить семпл с интерполяцией по позиции (0..size)
    pub fn get_interpolated(&self, position: f32) -> f32 {
        let pos_floor = position.floor() as usize;
        let frac = position.fract();
        
        let idx1 = pos_floor % self.size;
        let idx2 = (idx1 + 1) % self.size;
        
        let s1 = self.data[idx1];
        let s2 = self.data[idx2];
        
        s1 + frac * (s2 - s1)
    }
    
    /// Прочитать семпл с задержкой
    pub fn read_delayed(&self, delay_samples: usize, offset: usize) -> f32 {
        let available = if !self.filled {
            self.write_pos
        } else {
            self.size
        };
        
        let delay = delay_samples.min(available);
        let pos = (self.write_pos + self.size - delay - offset) % self.size;
        self.data[pos]
    }
    
    /// Прочитать семпл с задержкой и интерполяцией
    pub fn read_delayed_interpolated(&self, delay_samples: f32, offset: usize) -> f32 {
        let available = if !self.filled {
            self.write_pos
        } else {
            self.size
        } as f32;
        
        let delay = delay_samples.min(available - 0.001);
        
        // Вычисляем позицию для чтения с учетом задержки
        let mut read_pos = self.write_pos as f32 - delay - offset as f32;
        
        // Нормализуем в диапазон [0, size)
        while read_pos < 0.0 {
            read_pos += self.size as f32;
        }
        while read_pos >= self.size as f32 {
            read_pos -= self.size as f32;
        }
        
        let idx1 = read_pos.floor() as usize;
        let idx2 = (idx1 + 1) % self.size;
        let frac = read_pos.fract();
        
        let s1 = self.data[idx1];
        let s2 = self.data[idx2];
        
        s1 + frac * (s2 - s1)
    }
    
    /// Прочитать последовательность семплов с интерполяцией (для ring_buffer.read_interpolated)
    pub fn read_sequence_interpolated(&self, start_delay: f32, output: &mut [f32]) {
        let available = if !self.filled {
            self.write_pos
        } else {
            self.size
        } as f32;
        
        let delay = start_delay.min(available - 0.001);
        
        // Вычисляем начальную позицию (точка в прошлом, от которой начинаем читать)
        let start_pos = self.write_pos as f32 - delay;
        
        for i in 0..output.len() {
            // Для каждого семпла движемся вперед от начальной позиции
            let current_pos = start_pos + i as f32;
            
            // Нормализуем
            let mut pos = current_pos;
            while pos < 0.0 {
                pos += self.size as f32;
            }
            while pos >= self.size as f32 {
                pos -= self.size as f32;
            }
            
            let idx1 = pos.floor() as usize;
            let idx2 = (idx1 + 1) % self.size;
            let frac = pos.fract();
            
            let s1 = self.data[idx1];
            let s2 = self.data[idx2];
            
            output[i] = s1 + frac * (s2 - s1);
        }
    }
    
    /// Получить размер буфера
    pub fn size(&self) -> usize {
        self.size
    }
    
    /// Получить итератор по семплам
    pub fn iter(&self) -> BufferIterator<'_, '_> {
        BufferIterator {
            view: self,
            index: 0,
        }
    }
}

/// Представление буфера для записи (мутабельное)
pub struct BufferViewMut<'a> {
    data: RwLockWriteGuard<'a, Vec<f32>>,
    size: usize,
    write_pos: *mut usize,
    mask: usize,
    filled: *mut bool,
}

impl<'a> BufferViewMut<'a> {
    /// Создать новое мутабельное представление
    pub fn new(buffer: &'a mut RingBuffer) -> Self {
        let size = buffer.size();
        let mask = buffer.mask();
        
        let write_pos_ptr = &mut buffer.write_pos as *mut usize;
        let filled_ptr = &mut buffer.filled as *mut bool;
        
        Self {
            data: buffer.write_guard(),
            size,
            write_pos: write_pos_ptr,
            mask,
            filled: filled_ptr,
        }
    }
    
    /// Записать семпл в буфер
    pub fn write(&mut self, sample: f32) {
        let write_pos = unsafe { &mut *self.write_pos };
        let pos = *write_pos;
        
        self.data[pos] = sample;
        *write_pos = (pos + 1) & self.mask;
        
        let filled = unsafe { &mut *self.filled };
        if !*filled && *write_pos == 0 {
            *filled = true;
        }
    }
    
    /// Записать несколько семплов
    pub fn write_slice(&mut self, samples: &[f32]) {
        for &sample in samples {
            self.write(sample);
        }
    }
    
    /// Получить мутабельную ссылку на семпл по индексу
    pub fn get_mut(&mut self, index: usize) -> &mut f32 {
        &mut self.data[index % self.size]
    }
    
    /// Очистить буфер
    pub fn clear(&mut self) {
        self.data.fill(0.0);
        
        let write_pos = unsafe { &mut *self.write_pos };
        *write_pos = 0;
        
        let filled = unsafe { &mut *self.filled };
        *filled = false;
    }
}

/// Итератор по семплам буфера
pub struct BufferIterator<'a, 'b> {
    view: &'b BufferView<'a>,
    index: usize,
}

impl<'a, 'b> Iterator for BufferIterator<'a, 'b> {
    type Item = f32;
    
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.view.size() {
            let value = self.view.get(self.index);
            self.index += 1;
            Some(value)
        } else {
            None
        }
    }
}

impl<'a, 'b> ExactSizeIterator for BufferIterator<'a, 'b> {
    fn len(&self) -> usize {
        self.view.size() - self.index
    }
}