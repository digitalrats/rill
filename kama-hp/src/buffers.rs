//! # Высокоточные буферы
//! 
//! Предоставляет [`HighPrecisionBuffer`] для работы с аудиоданными в формате `f64`
//! и интеграцию с [`kama-buffers`] через [`HighPrecisionBufferPool`].
//! 
//! ## Особенности
//! 
//! - Поддержка нескольких каналов (interleaved формат)
//! - Интерполяция при чтении с дробной позицией
//! - Конвертация между f32 и f64
//! - Интеграция с пулом буферов из `kama-buffers`

use kama_buffers::{BufferManager, PooledBuffer};

/// Высокоточный аудиобуфер (f64).
/// 
/// Хранит данные в interleaved формате: `[L, R, L, R, ...]`.
#[derive(Debug, Clone)]
pub struct HighPrecisionBuffer {
    data: Vec<f64>,
    size: usize,
    channels: usize,
    sample_rate: f64,
}

impl HighPrecisionBuffer {
    /// Создать новый буфер.
    /// 
    /// # Аргументы
    /// * `size` — количество фреймов
    /// * `channels` — количество каналов
    /// * `sample_rate` — частота дискретизации
    pub fn new(size: usize, channels: usize, sample_rate: f64) -> Self {
        Self {
            data: vec![0.0; size * channels],
            size,
            channels,
            sample_rate,
        }
    }
    
    /// Создать из PooledBuffer (из kama-buffers).
    pub fn from_pooled_buffer(buffer: &PooledBuffer, channels: usize, sample_rate: f64) -> Self {
        let frames = buffer.len() / channels;
        let mut hp_buffer = Self::new(frames, channels, sample_rate);
        
        for (i, &sample) in buffer.as_slice().iter().enumerate() {
            if i < hp_buffer.data.len() {
                hp_buffer.data[i] = sample as f64;
            }
        }
        
        hp_buffer
    }
    
    /// Создать из f32 слайса.
    pub fn from_f32_slice(data: &[f32], channels: usize, sample_rate: f64) -> Self {
        let frames = data.len() / channels;
        let mut buffer = Self::new(frames, channels, sample_rate);
        
        for (i, &sample) in data.iter().enumerate() {
            if i < buffer.data.len() {
                buffer.data[i] = sample as f64;
            }
        }
        
        buffer
    }
    
    /// Записать значение в позицию.
    pub fn write(&mut self, position: usize, channel: usize, value: f64) {
        let idx = position * self.channels + channel;
        if idx < self.data.len() {
            self.data[idx] = value;
        }
    }
    
    /// Записать целый блок данных (interleaved).
    pub fn write_block(&mut self, start_frame: usize, data: &[f64]) {
        let start_idx = start_frame * self.channels;
        let end_idx = (start_idx + data.len()).min(self.data.len());
        
        for (i, &val) in data.iter().enumerate() {
            let idx = start_idx + i;
            if idx < end_idx {
                self.data[idx] = val;
            }
        }
    }
    
    /// Записать целый блок из f32 (interleaved).
    pub fn write_block_f32(&mut self, start_frame: usize, data: &[f32]) {
        let start_idx = start_frame * self.channels;
        let end_idx = (start_idx + data.len()).min(self.data.len());
        
        for (i, &val) in data.iter().enumerate() {
            let idx = start_idx + i;
            if idx < end_idx {
                self.data[idx] = val as f64;
            }
        }
    }
    
    /// Прочитать значение из позиции.
    pub fn read(&self, position: usize, channel: usize) -> f64 {
        let idx = position * self.channels + channel;
        self.data.get(idx).copied().unwrap_or(0.0)
    }
    
    /// Прочитать целый канал.
    pub fn read_channel(&self, channel: usize) -> Vec<f64> {
        let mut result = Vec::with_capacity(self.size);
        for frame in 0..self.size {
            result.push(self.read(frame, channel));
        }
        result
    }
    
    /// Прочитать с интерполяцией.
    pub fn read_interpolated(&self, position: f64, channel: usize) -> f64 {
        let pos_floor = position.floor() as usize;
        let frac = position.fract();
        
        let idx1 = pos_floor * self.channels + channel;
        let idx2 = ((pos_floor + 1) % self.size) * self.channels + channel;
        
        let s1 = self.data.get(idx1).copied().unwrap_or(0.0);
        let s2 = self.data.get(idx2).copied().unwrap_or(0.0);
        
        s1 + frac * (s2 - s1)
    }
    
    /// Прочитать блок данных (interleaved).
    pub fn read_block(&self, start_frame: usize, num_frames: usize) -> Vec<f64> {
        let start_idx = start_frame * self.channels;
        let end_idx = (start_idx + num_frames * self.channels).min(self.data.len());
        
        self.data[start_idx..end_idx].to_vec()
    }
    
    /// Прочитать блок и сконвертировать в f32.
    pub fn read_block_f32(&self, start_frame: usize, num_frames: usize) -> Vec<f32> {
        self.read_block(start_frame, num_frames)
            .iter()
            .map(|&x| x as f32)
            .collect()
    }
    
    /// Конвертировать в f32 вектор (interleaved).
    pub fn to_f32(&self) -> Vec<f32> {
        self.data.iter().map(|&x| x as f32).collect()
    }
    
    /// Конвертировать в f32 вектор и записать в PooledBuffer.
    pub fn copy_to_pooled_buffer(&self, pooled: &mut PooledBuffer) -> usize {
        let f32_data = self.to_f32();
        let copy_len = f32_data.len().min(pooled.len());
        pooled.as_mut_slice()[..copy_len].copy_from_slice(&f32_data[..copy_len]);
        copy_len
    }
    
    /// Конвертировать в f32 вектор, разделённый по каналам.
    pub fn to_f32_deinterleaved(&self) -> Vec<Vec<f32>> {
        let mut result = vec![Vec::with_capacity(self.size); self.channels];
        
        for frame in 0..self.size {
            for ch in 0..self.channels {
                result[ch].push(self.read(frame, ch) as f32);
            }
        }
        
        result
    }
    
    /// Заполнить буфер из f32 вектора, разделённого по каналам.
    pub fn from_f32_deinterleaved(data: &[Vec<f32>], sample_rate: f64) -> Self {
        let channels = data.len();
        let size = if channels > 0 { data[0].len() } else { 0 };
        
        let mut buffer = Self::new(size, channels, sample_rate);
        
        for ch in 0..channels {
            for frame in 0..size.min(data[ch].len()) {
                buffer.write(frame, ch, data[ch][frame] as f64);
            }
        }
        
        buffer
    }
    
    /// Применить функцию к каждому сэмплу.
    pub fn apply<F>(&mut self, mut f: F)
    where
        F: FnMut(f64) -> f64,
    {
        for sample in &mut self.data {
            *sample = f(*sample);
        }
    }
    
    /// Применить функцию к каждому сэмплу с учётом позиции и канала.
    pub fn apply_with_pos<F>(&mut self, mut f: F)
    where
        F: FnMut(usize, usize, f64) -> f64,
    {
        for frame in 0..self.size {
            for ch in 0..self.channels {
                let idx = frame * self.channels + ch;
                self.data[idx] = f(frame, ch, self.data[idx]);
            }
        }
    }
    
    /// Очистить буфер (заполнить нулями).
    pub fn clear(&mut self) {
        self.data.fill(0.0);
    }
    
    /// Получить размер во фреймах.
    pub fn size(&self) -> usize {
        self.size
    }
    
    /// Получить количество каналов.
    pub fn channels(&self) -> usize {
        self.channels
    }
    
    /// Получить частоту дискретизации.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }
    
    /// Получить общее количество сэмплов.
    pub fn total_samples(&self) -> usize {
        self.data.len()
    }
    
    /// Получить сырые данные (для продвинутого использования).
    pub fn raw_data(&self) -> &[f64] {
        &self.data
    }
    
    /// Получить мутабельные сырые данные.
    pub fn raw_data_mut(&mut self) -> &mut [f64] {
        &mut self.data
    }
}

/// Пул высокоточных буферов.
/// 
/// Обёртка над [`BufferManager`](kama_buffers::BufferManager) для работы с f64 буферами.
pub struct HighPrecisionBufferPool {
    manager: BufferManager,
    channels: usize,
    sample_rate: f64,
}

impl HighPrecisionBufferPool {
    /// Создать новый пул.
    pub fn new(manager: BufferManager, channels: usize, sample_rate: f64) -> Self {
        Self {
            manager,
            channels,
            sample_rate,
        }
    }
    
    /// Получить буфер из пула.
    pub fn acquire(&mut self, frames: usize) -> Option<HighPrecisionBuffer> {
        let total_samples = frames * self.channels;
        self.manager.acquire(total_samples).ok().map(|pooled| {
            HighPrecisionBuffer::from_pooled_buffer(&pooled, self.channels, self.sample_rate)
        })
    }
    
    /// Вернуть буфер в пул.
    pub fn release(&mut self, buffer: HighPrecisionBuffer) {
        // Конвертируем обратно в f32 и возвращаем в менеджер
        if let Ok(mut pooled) = self.manager.acquire(buffer.total_samples()) {
            buffer.copy_to_pooled_buffer(&mut pooled);
            // pooled автоматически вернётся в пул при drop
        }
    }
    
    /// Получить доступ к внутреннему менеджеру.
    pub fn manager(&self) -> &BufferManager {
        &self.manager
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_buffer_basic() {
        let mut buffer = HighPrecisionBuffer::new(10, 2, 44100.0);
        buffer.write(0, 0, 0.5);
        buffer.write(0, 1, -0.5);
        
        assert_eq!(buffer.read(0, 0), 0.5);
        assert_eq!(buffer.read(0, 1), -0.5);
        assert_eq!(buffer.size(), 10);
        assert_eq!(buffer.channels(), 2);
    }
    
    #[test]
    fn test_buffer_interpolated() {
        let mut buffer = HighPrecisionBuffer::new(4, 1, 44100.0);
        buffer.write(0, 0, 0.0);
        buffer.write(1, 0, 1.0);
        buffer.write(2, 0, 2.0);
        buffer.write(3, 0, 3.0);
        
        assert_eq!(buffer.read_interpolated(0.5, 0), 0.5);
        assert_eq!(buffer.read_interpolated(1.2, 0), 1.2);
        assert_eq!(buffer.read_interpolated(2.8, 0), 2.8);
    }
    
    #[test]
    fn test_buffer_clear() {
        let mut buffer = HighPrecisionBuffer::new(5, 2, 44100.0);
        buffer.write(0, 0, 0.5);
        buffer.clear();
        assert_eq!(buffer.read(0, 0), 0.0);
    }
    
    #[test]
    fn test_buffer_f32_conversion() {
        let mut buffer = HighPrecisionBuffer::new(10, 2, 44100.0);
        buffer.write(0, 0, 0.5);
        buffer.write(0, 1, -0.5);
        
        let f32_data = buffer.to_f32();
        assert_eq!(f32_data.len(), 20);
        assert!((f32_data[0] - 0.5).abs() < 1e-6);
        assert!((f32_data[1] + 0.5).abs() < 1e-6);
    }
}