use std::sync::Arc;
use parking_lot::RwLock;

/// Кольцевой буфер с фиксированным размером
/// Всегда содержит последние size записанных семплов
#[derive(Clone, Debug)]
pub struct RingBuffer {
    buffer: Arc<RwLock<Vec<f32>>>,
    write_pos: usize,
    size: usize,
    mask: usize,
    filled: bool,  // стал ли буфер полностью заполненным хотя бы раз
}

impl RingBuffer {
    pub fn new(size: usize) -> Self {
        let size = size.next_power_of_two();
        Self {
            buffer: Arc::new(RwLock::new(vec![0.0; size])),
            write_pos: 0,
            size,
            mask: size - 1,
            filled: false,
        }
    }
    
    pub fn write(&mut self, samples: &[f32]) {
        let mut buffer = self.buffer.write();
        let mut pos = self.write_pos;
        
        for &sample in samples {
            buffer[pos] = sample;
            pos = (pos + 1) & self.mask;
        }
        
        self.write_pos = pos;
        
        // Если мы хотя бы раз заполнили весь буфер, отмечаем это
        if !self.filled && self.write_pos == 0 {
            self.filled = true;
        }
    }
    
    pub fn read(&self, delay_samples: usize, output: &mut [f32]) {
        let buffer = self.buffer.read();
        
        // Если буфер ещё не полностью заполнен, читаем только реальные данные
        let available = if !self.filled {
            self.write_pos
        } else {
            self.size
        };
        
        // delay_samples не может быть больше доступных семплов
        let delay = delay_samples.min(available);
        
        for i in 0..output.len() {
            // Позиция: (write_pos - delay - i) mod size
            // Используем сложение с size для избежания отрицательных чисел
            let pos = (self.write_pos + self.size - delay - i) % self.size;
            output[i] = buffer[pos];
        }
    }
    
pub fn read_interpolated(&self, delay_samples: f32, output: &mut [f32]) {
    let buffer = self.buffer.read();
    
    // Если буфер ещё не полностью заполнен, используем только реальные данные
    let available = if !self.filled {
        self.write_pos
    } else {
        self.size
    } as f32;
    
    // Нормализуем задержку, чтобы она не превышала доступные данные
    let delay = delay_samples.min(available - 0.001);
    
    // Вычисляем начальную позицию для первого семпла
    let mut read_pos = self.write_pos as f32 - delay;
    
    // Нормализуем начальную позицию в диапазон [0, size)
    while read_pos < 0.0 {
        read_pos += self.size as f32;
    }
    while read_pos >= self.size as f32 {
        read_pos -= self.size as f32;
    }
    
    for (i, out) in output.iter_mut().enumerate() {
        // Текущая позиция с учётом обёртывания
        let current_pos = read_pos + i as f32;
        
        // Нормализуем в диапазон [0, size)
        let mut pos = current_pos;
        while pos >= self.size as f32 {
            pos -= self.size as f32;
        }
        
        // Находим два ближайших целых индекса для интерполяции
        let idx1 = pos.floor() as usize;
        let idx2 = (idx1 + 1) % self.size;
        let frac = pos.fract();
        
        let s1 = buffer[idx1];
        let s2 = buffer[idx2];
        
        *out = s1 + frac * (s2 - s1);
        
        println!("i={}, pos={:.2}, idx1={}, idx2={}, frac={:.2}, s1={}, s2={}, out={}", 
                 i, pos, idx1, idx2, frac, s1, s2, *out);
    }
}
    
    pub fn size(&self) -> usize {
        self.size
    }
    
    /// Получить количество реально записанных семплов
    pub fn len(&self) -> usize {
        if self.filled {
            self.size
        } else {
            self.write_pos
        }
    }
    
    /// Получить значение по индексу (для внутреннего использования в крейте)
    pub(crate) fn get(&self, index: usize) -> f32 {
        let buffer = self.buffer.read();
        buffer[index & self.mask]
    }
    
    /// Получить доступ к данным для чтения (возвращает guard)
    pub(crate) fn read_guard(&self) -> parking_lot::RwLockReadGuard<'_, Vec<f32>> {
        self.buffer.read()
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
        // Теперь буфер должен содержать [5,6,3,4]? Нет, последние 4 семпла: 3,4,5,6
        
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
}