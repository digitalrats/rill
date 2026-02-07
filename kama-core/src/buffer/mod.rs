use std::sync::Arc;
use parking_lot::RwLock;

/// Кольцевой буфер для аудио
pub struct RingBuffer {
    buffer: Arc<RwLock<Vec<f32>>>,
    write_pos: usize,
    size: usize,
    mask: usize,
}

impl RingBuffer {
    pub fn new(size: usize) -> Self {
        let size = size.next_power_of_two();
        Self {
            buffer: Arc::new(RwLock::new(vec![0.0; size])),
            write_pos: 0,
            size,
            mask: size - 1,
        }
    }
    
    pub fn write(&mut self, samples: &[f32]) {
        let mut buffer = self.buffer.write();
        let pos = self.write_pos;
        
        for (i, &sample) in samples.iter().enumerate() {
            buffer[(pos + i) & self.mask] = sample;
        }
        
        self.write_pos = (pos + samples.len()) & self.mask;
    }
    
    pub fn read(&self, delay_samples: usize, output: &mut [f32]) {
        let buffer = self.buffer.read();
        let read_pos = (self.write_pos.wrapping_sub(delay_samples)) & self.mask;
        
        for i in 0..output.len() {
            output[i] = buffer[(read_pos + i) & self.mask];
        }
    }
    
    pub fn read_interpolated(&self, delay_samples: f32, output: &mut [f32]) {
        let buffer = self.buffer.read();
        
        for (i, out) in output.iter_mut().enumerate() {
            let delay = delay_samples + i as f32;
            let index_f = delay.floor();
            let frac = delay.fract();
            
            let idx1 = (self.write_pos.wrapping_sub(index_f as usize + 1)) & self.mask;
            let idx2 = (self.write_pos.wrapping_sub(index_f as usize)) & self.mask;
            
            let s1 = buffer[idx1];
            let s2 = buffer[idx2];
            
            *out = s1 + frac * (s2 - s1);
        }
    }
}

/// Пул буферов для предотвращения аллокаций
pub struct BufferPool {
    buffers: Vec<Vec<f32>>,
    size: usize,
}

impl BufferPool {
    pub fn new(pool_size: usize, buffer_size: usize) -> Self {
        let mut buffers = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            buffers.push(vec![0.0; buffer_size]);
        }
        
        Self { buffers, size: buffer_size }
    }
    
    pub fn acquire(&mut self) -> Option<Vec<f32>> {
        self.buffers.pop()
    }
    
    pub fn release(&mut self, mut buffer: Vec<f32>) {
        if buffer.len() == self.size {
            buffer.fill(0.0);
            self.buffers.push(buffer);
        }
    }
}
