use crate::processor::SampleProcessor;

/// Состояние головки воспроизведения
#[derive(Debug, Clone, Copy)]
pub struct HeadState {
    pub current_position: usize,
    pub speed: f32,
    pub direction: Direction,
    pub volume: f32,
    pub pan: f32,
}

/// Направление воспроизведения
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Reverse,
}

/// Режим чтения буфера
#[derive(Debug, Clone, Copy)]
pub enum ReadMode {
    Simple,          // Простое чтение
    Granular {       // Гранулярный синтез
        grain_size: usize,
        grain_spacing: usize,
        randomization: f32,
    },
    Reverse,         // Обратное воспроизведение
    PingPong {       // Вперёд-назад
        segment_size: usize,
    },
}

/// Головка воспроизведения
#[derive(Clone)]
pub struct BufferHead {
    pub state: HeadState,
    pub read_mode: ReadMode,
    pub processor: SampleProcessor,
    pub enabled: bool,
    pub id: usize,
}

impl BufferHead {
    pub fn new(id: usize) -> Self {
        Self {
            state: HeadState {
                current_position: 0,
                speed: 1.0,
                direction: Direction::Forward,
                volume: 1.0,
                pan: 0.0,
            },
            read_mode: ReadMode::Simple,
            processor: SampleProcessor::None,
            enabled: true,
            id,
        }
    }
    
    pub fn with_speed(mut self, speed: f32) -> Self {
        self.state.speed = speed;
        self
    }
    
    pub fn with_pan(mut self, pan: f32) -> Self {
        self.state.pan = pan.max(-1.0).min(1.0);
        self
    }
    
    pub fn with_gain(mut self, gain: f32) -> Self {
        self.processor = SampleProcessor::Gain(gain.max(0.0));
        self
    }
    
    pub fn with_lfo(mut self, frequency: f32, amplitude: f32) -> Self {
        self.processor = SampleProcessor::Lfo {
            frequency,
            amplitude,
            phase: 0.0,
        };
        self
    }
}