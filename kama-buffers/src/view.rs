/// Представление буфера для обработки
pub struct BufferView<'a> {
    pub(crate) data: &'a [f32],
    pub(crate) size: usize,
}

impl<'a> BufferView<'a> {
    pub fn new(data: &'a [f32], size: usize) -> Self {
        Self { data, size }
    }
    
    pub fn get(&self, pos: usize) -> f32 {
        self.data[pos % self.size]
    }
    
    pub fn get_interpolated(&self, pos: f32) -> f32 {
        let pos_floor = pos.floor();
        let frac = pos.fract();
        
        let idx1 = pos_floor as usize % self.size;
        let idx2 = (idx1 + 1) % self.size;
        
        let s1 = self.data[idx1];
        let s2 = self.data[idx2];
        
        s1 + frac * (s2 - s1)
    }
    
    pub fn size(&self) -> usize {
        self.size
    }
}