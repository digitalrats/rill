use std::sync::Arc;
use parking_lot::RwLock;

/// Высокоточный аудиобуфер (f64)
#[derive(Debug, Clone)]
pub struct HighPrecisionBuffer {
    data: Arc<RwLock<Vec<f64>>>,
    size: usize,
    channels: usize,
    sample_rate: f64,
}

impl HighPrecisionBuffer {
    pub fn new(size: usize, channels: usize, sample_rate: f64) -> Self {
        Self {
            data: Arc::new(RwLock::new(vec![0.0; size * channels])),
            size,
            channels,
            sample_rate,
        }
    }
    
    pub fn from_f32(data: &[f32], channels: usize, sample_rate: f64) -> Self {
        let size = data.len() / channels;
        let mut buffer = Self::new(size, channels, sample_rate);
        
        {
            let mut data_f64 = buffer.data.write();
            for (i, &sample) in data.iter().enumerate() {
                if i < data_f64.len() {
                    data_f64[i] = sample as f64;
                }
            }
        } // блокировка освобождается здесь
        
        buffer
    }
    
    pub fn write(&mut self, position: usize, channel: usize, value: f64) {
        let mut data = self.data.write();
        let idx = (position % self.size) * self.channels + channel;
        if idx < data.len() {
            data[idx] = value;
        }
    }
    
    pub fn read(&self, position: usize, channel: usize) -> f64 {
        let data = self.data.read();
        let idx = (position % self.size) * self.channels + channel;
        data.get(idx).copied().unwrap_or(0.0)
    }
    
    pub fn read_interpolated(&self, position: f64, channel: usize) -> f64 {
        let pos_floor = position.floor();
        let pos_frac = position.fract();
        
        let data = self.data.read();
        let idx1 = (pos_floor as usize % self.size) * self.channels + channel;
        let idx2 = ((pos_floor as usize + 1) % self.size) * self.channels + channel;
        
        let sample1 = data.get(idx1).copied().unwrap_or(0.0);
        let sample2 = data.get(idx2).copied().unwrap_or(0.0);
        
        sample1 + pos_frac * (sample2 - sample1)
    }
    
    pub fn to_f32(&self) -> Vec<f32> {
        let data = self.data.read();
        data.iter().map(|&x| x as f32).collect()
    }
    
    pub fn convert_from_f32(&mut self, data: &[f32]) {
        let mut buffer = self.data.write();
        for (i, &sample) in data.iter().enumerate() {
            if i < buffer.len() {
                buffer[i] = sample as f64;
            }
        }
    }
    
    pub fn convert_to_f32(&self, output: &mut [f32]) {
        let data = self.data.read();
        for (i, &sample) in data.iter().enumerate() {
            if i < output.len() {
                output[i] = sample as f32;
            }
        }
    }
    
    pub fn size(&self) -> usize {
        self.size
    }
    
    pub fn channels(&self) -> usize {
        self.channels
    }
    
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }
}

/// Пул высокоточных буферов для повторного использования.
pub struct HighPrecisionBufferPool {
    buffers: Vec<HighPrecisionBuffer>,
    buffer_size: usize,
    channels: usize,
    sample_rate: f64,
}

impl HighPrecisionBufferPool {
    pub fn new(pool_size: usize, buffer_size: usize, channels: usize, sample_rate: f64) -> Self {
        let mut buffers = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            buffers.push(HighPrecisionBuffer::new(buffer_size, channels, sample_rate));
        }
        
        Self {
            buffers,
            buffer_size,
            channels,
            sample_rate,
        }
    }
    
    pub fn acquire(&mut self) -> Option<HighPrecisionBuffer> {
        self.buffers.pop()
    }
    
    pub fn release(&mut self, buffer: HighPrecisionBuffer) {
        if buffer.size == self.buffer_size && buffer.channels == self.channels {
            {
                let mut data = buffer.data.write();
                data.fill(0.0);
            } // блокировка освобождается
            self.buffers.push(buffer);
        }
    }
}