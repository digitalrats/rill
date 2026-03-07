//! Tick information for musical timing

/// Information about the current musical position
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickInfo {
    /// Bar number (starting from 0)
    pub bar: u32,
    
    /// Beat within the bar (0-3, where 0 is the first beat)
    pub beat: u8,
    
    /// Sixteenth note within the beat (0-3)
    pub sixteenth: u8,
    
    /// Absolute sample position
    pub sample_pos: u64,
}

impl TickInfo {
    /// Create a new TickInfo
    pub fn new(bar: u32, beat: u8, sixteenth: u8, sample_pos: u64) -> Self {
        Self {
            bar,
            beat,
            sixteenth,
            sample_pos,
        }
    }
    
    /// Check if this is the start of a new bar
    pub fn is_new_bar(&self) -> bool {
        self.beat == 0 && self.sixteenth == 0
    }
    
    /// Check if this is the start of a new beat
    pub fn is_new_beat(&self) -> bool {
        self.sixteenth == 0
    }
}