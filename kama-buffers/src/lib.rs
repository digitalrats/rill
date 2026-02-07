use std::sync::Arc;
use parking_lot::RwLock;
use thiserror::Error;
use rand::Rng;

// Импорты из kama-core
use kama_core::AudioNode;
use kama_core::AudioError;
use kama_core::param::{ParamValue, ParamType};
use kama_core::node::{NodeMetadata, NodeCategory, ParamMetadata};

#[derive(Error, Debug)]
pub enum BufferError {
    #[error("Invalid head ID: {0}")]
    InvalidHeadId(usize),
    #[error("Buffer full")]
    BufferFull,
    #[error("Buffer empty")]
    BufferEmpty,
    #[error("Invalid parameter: {0}")]
    InvalidParameter(String),
}

pub type AudioBuffer = Vec<f32>;
pub type BufferResult<T> = Result<T, BufferError>;

// === Базовый кольцевой буфер ===

/// Кольцевой буфер с фиксированным размером
#[derive(Clone, Debug)]
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
    
    pub fn size(&self) -> usize {
        self.size
    }
}

// === Многоголовый буфер ===

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

/// Представление буфера для обработки
pub struct BufferView<'a> {
    data: &'a [f32],
    size: usize,
}

impl<'a> BufferView<'a> {
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

pub struct HeadProcessor {
    process_func: Box<dyn Fn(f32, &HeadState, &BufferView) -> f32 + Send + Sync>,
}

impl Clone for HeadProcessor {
    fn clone(&self) -> Self {
        Self {
            process_func: Box::new(|sample, _state, _view| sample),
        }
    }
}

/// Обработчик семпла для головки
pub enum SampleProcessor {
    None,
    Gain(f32),
    Pan(f32), // -1.0 (лево) до 1.0 (право)
    Lfo {
        frequency: f32,
        amplitude: f32,
        phase: f32,
    },
    Custom(HeadProcessor),
}

impl Clone for SampleProcessor {
    fn clone(&self) -> Self {
        match self {
            SampleProcessor::None => SampleProcessor::None,
            SampleProcessor::Gain(g) => SampleProcessor::Gain(*g),
            SampleProcessor::Pan(p) => SampleProcessor::Pan(*p),
            SampleProcessor::Lfo { frequency, amplitude, phase } => 
                SampleProcessor::Lfo {
                    frequency: *frequency,
                    amplitude: *amplitude,
                    phase: *phase,
                },
            SampleProcessor::Custom(proc) => SampleProcessor::Custom(proc.clone()),
        }
    }
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

/// Многоголовый буфер для сложного воспроизведения
#[derive(Clone)]
pub struct MultiHeadBuffer {
    buffer: RingBuffer,
    heads: Vec<BufferHead>,
    sample_rate: f32,
    max_heads: usize,
}

impl MultiHeadBuffer {
    pub fn new(size: usize, sample_rate: f32) -> Self {
        Self {
            buffer: RingBuffer::new(size),
            heads: Vec::new(),
            sample_rate,
            max_heads: 8,
        }
    }
    
    pub fn add_head(&mut self) -> usize {
        if self.heads.len() >= self.max_heads {
            return 0;
        }
        
        let id = self.heads.len() + 1;
        let head = BufferHead::new(id);
        self.heads.push(head);
        id
    }
    
    pub fn remove_head(&mut self, id: usize) -> BufferResult<()> {
        if id == 0 || id > self.heads.len() {
            return Err(BufferError::InvalidHeadId(id));
        }
        
        self.heads.remove(id - 1);
        for (i, head) in self.heads.iter_mut().enumerate() {
            head.id = i + 1;
        }
        
        Ok(())
    }
    
    pub fn get_head(&self, id: usize) -> Option<&BufferHead> {
        if id == 0 || id > self.heads.len() {
            return None;
        }
        self.heads.get(id - 1)
    }
    
    pub fn get_head_mut(&mut self, id: usize) -> Option<&mut BufferHead> {
        if id == 0 || id > self.heads.len() {
            return None;
        }
        self.heads.get_mut(id - 1)
    }
    
    pub fn write(&mut self, samples: &[f32]) {
        self.buffer.write(samples);
    }
    
    pub fn buffer_size(&self) -> usize {
        self.buffer.size()
    }
    
/// Основной метод обработки (использует оптимизированную версию)
pub fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), BufferError> {
    self.process_optimized(inputs, outputs)
} 

    /// Статический метод обработки головки (без доступа к self)
 /// Статический метод обработки головки (без доступа к self)
fn process_head_static(
    head_idx: usize,
    state: &mut HeadState,
    read_mode: &ReadMode,
    processor: &SampleProcessor,
    pan: &mut f32,
    buffer_size: usize,
    sample_rate: f32,
    view: &BufferView,
    output_len: usize,
    outputs: &mut [&mut [f32]]
) {
    match read_mode {
            ReadMode::Simple => {
                for i in 0..output_len {
                    let sample = Self::read_sample_static(state, view);
                    let processed = Self::process_sample_static(sample, state, processor, sample_rate, view);
                    Self::write_to_outputs_static(processed, *pan, i, outputs);
                    
                    state.current_position = (state.current_position + 1) % buffer_size;
                }
            }
            ReadMode::Granular { grain_size, grain_spacing, randomization } => {
                let mut grain_pos = 0;
                let mut in_grain = false;
                
                for i in 0..output_len {
                    if !in_grain || grain_pos >= *grain_size {
                        let mut rng = rand::thread_rng();
                        state.current_position = ((state.current_position as f32 + 
                            randomization * (rng.gen::<f32>() * 2.0 - 1.0) * buffer_size as f32) as usize) % buffer_size;
                        grain_pos = 0;
                        in_grain = true;
                    }
                    
                    if grain_pos < *grain_size {
                        let window = Self::grain_window_static(grain_pos, *grain_size);
                        let sample = Self::read_sample_static(state, view);
                        let processed = Self::process_sample_static(sample * window, state, processor, sample_rate, view);
                        Self::write_to_outputs_static(processed, *pan, i, outputs);
                        
                        grain_pos += 1;
                    } else {
                        Self::write_to_outputs_static(0.0, *pan, i, outputs);
                    }
                    
                    state.current_position = (state.current_position + 1) % buffer_size;
                }
            }
            ReadMode::Reverse => {
                for i in 0..output_len {
                    let rev_pos = (buffer_size - state.current_position) % buffer_size;
                    let sample = view.get(rev_pos);
                    let processed = Self::process_sample_static(sample, state, processor, sample_rate, view);
                    Self::write_to_outputs_static(processed, *pan, i, outputs);
                    
                    state.current_position = (state.current_position + 1) % buffer_size;
                }
            }
            ReadMode::PingPong { segment_size } => {
                let mut direction_forward = true;
                let mut segment_pos = 0;
                
                for i in 0..output_len {
                    let sample = Self::read_sample_static(state, view);
                    let processed = Self::process_sample_static(sample, state, processor, sample_rate, view);
                    Self::write_to_outputs_static(processed, *pan, i, outputs);
                    
                    segment_pos += 1;
                    
                    if segment_pos >= *segment_size {
                        direction_forward = !direction_forward;
                        segment_pos = 0;
                    }
                    
                    if direction_forward {
                        state.current_position = (state.current_position + 1) % buffer_size;
                    } else {
                        state.current_position = (state.current_position + buffer_size - 1) % buffer_size;
                    }
                }
            }
        }
    }
    
    fn read_sample_static(state: &HeadState, view: &BufferView) -> f32 {
        let pos = state.current_position as f32 * state.speed;
        view.get_interpolated(pos)
    }
    
    fn process_sample_static(sample: f32, state: &HeadState, processor: &SampleProcessor, sample_rate: f32, view: &BufferView) -> f32 {
        let mut result = sample * state.volume;
        
        match processor {
            SampleProcessor::None => {},
            SampleProcessor::Gain(gain) => {
                result *= *gain;
            }
            SampleProcessor::Pan(_) => {
                // Панорамирование применяется при записи в outputs
            }
            SampleProcessor::Lfo { frequency, amplitude, phase } => {
                let time = state.current_position as f32 / sample_rate;
                let lfo = (2.0 * std::f32::consts::PI * *frequency * time + *phase).sin();
                result *= 1.0 + lfo * *amplitude;
            }
            SampleProcessor::Custom(processor) => {
                result = (processor.process_func)(result, state, view);
            }
        }
        
        result
    }

     pub fn process_with_storage(
        &mut self,
        inputs: &[&[f32]],
        output_storage: &mut [f32],
        num_channels: usize
    ) -> Result<(), BufferError> {
        if output_storage.is_empty() || num_channels == 0 {
            return Ok(());
        }
        
        if !inputs.is_empty() {
            self.write(inputs[0]);
        }
        
        let buffer_size = output_storage.len() / num_channels;
        
        // Разделяем storage на каналы
        let channel_slices: Vec<&mut [f32]> = output_storage
            .chunks_mut(buffer_size)
            .take(num_channels)
            .collect();
        
        // Преобразуем в массив срезов
        let mut outputs: Vec<&mut [f32]> = channel_slices;
        
        self.process_optimized(inputs, &mut outputs)
    }
    
    fn grain_window_static(position: usize, grain_size: usize) -> f32 {
        let x = position as f32 / grain_size as f32;
        0.5 * (1.0 - (2.0 * std::f32::consts::PI * x).cos())
    }
    
    fn write_to_outputs_static(sample: f32, pan: f32, index: usize, outputs: &mut [&mut [f32]]) {
        if outputs.is_empty() {
            return;
        }
        
        if outputs.len() >= 2 {
            let (first, rest) = outputs.split_at_mut(1);
            let left_output = &mut first[0];
            let right_output = &mut rest[0];
            
            if index < left_output.len() && index < right_output.len() {
                let (left_gain, right_gain) = Self::pan_to_gains_static(pan);
                left_output[index] += sample * left_gain;
                right_output[index] += sample * right_gain;
            }
        } else {
            let output = &mut outputs[0];
            if index < output.len() {
                output[index] += sample;
            }
        }
    }
    
    fn pan_to_gains_static(pan: f32) -> (f32, f32) {
        let pan = pan.max(-1.0).min(1.0);
        let left_gain = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
        let right_gain = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
        (left_gain, right_gain)
    }
    
    /// Оптимизированная версия для производительности
    pub fn process_optimized(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), BufferError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        if !inputs.is_empty() {
            self.write(inputs[0]);
        }
        
        let buffer_size = self.buffer_size();
        let sample_rate = self.sample_rate;
        let buffer_data = self.buffer.buffer.read();
        let output_len = outputs[0].len();
        
        // Используем unsafe для максимальной производительности
        unsafe {
            let buffer_ptr = buffer_data.as_ptr();
            let buffer_mask = buffer_size - 1; // Предполагаем степень двойки
            
            for head_idx in 0..self.heads.len() {
                let head = &mut *self.heads.as_mut_ptr().add(head_idx);
                
                if !head.enabled {
                    continue;
                }
                
                let mut current_pos = head.state.current_position;
                let speed = head.state.speed;
                let volume = head.state.volume;
                let pan = head.state.pan;
                
                // Предвычисленные значения панорамирования
                let (left_gain, right_gain) = if pan <= 0.0 {
                    (1.0, 1.0 + pan)
                } else {
                    (1.0 - pan, 1.0)
                };
                
                // Быстрая обработка Simple режима
                if matches!(head.read_mode, ReadMode::Simple) {
                    if outputs.len() >= 2 {
                        let left_out = outputs[0].as_mut_ptr();
                        let right_out = outputs[1].as_mut_ptr();
                        
                        for i in 0..output_len {
                            let pos_f = (current_pos as f32 * speed) as f32;
                            let pos_floor = pos_f.floor() as usize;
                            let frac = pos_f.fract();
                            
                            let idx1 = (pos_floor & buffer_mask) % buffer_size;
                            let idx2 = ((pos_floor + 1) & buffer_mask) % buffer_size;
                            
                            let s1 = *buffer_ptr.add(idx1);
                            let s2 = *buffer_ptr.add(idx2);
                            let sample = s1 + frac * (s2 - s1);
                            let processed = sample * volume;
                            
                            *left_out.add(i) += processed * left_gain;
                            *right_out.add(i) += processed * right_gain;
                            
                            current_pos = (current_pos + 1) % buffer_size;
                        }
                    } else {
                        let mono_out = outputs[0].as_mut_ptr();
                        
                        for i in 0..output_len {
                            let pos_f = (current_pos as f32 * speed) as f32;
                            let pos_floor = pos_f.floor() as usize;
                            let frac = pos_f.fract();
                            
                            let idx1 = (pos_floor & buffer_mask) % buffer_size;
                            let idx2 = ((pos_floor + 1) & buffer_mask) % buffer_size;
                            
                            let s1 = *buffer_ptr.add(idx1);
                            let s2 = *buffer_ptr.add(idx2);
                            let sample = s1 + frac * (s2 - s1);
                            let processed = sample * volume;
                            
                            *mono_out.add(i) += processed;
                            
                            current_pos = (current_pos + 1) % buffer_size;
                        }
                    }
                    
                    head.state.current_position = current_pos;
                } else {
                    // Для сложных режимов используем безопасную версию
                    let view = BufferView {
                        data: &buffer_data,
                        size: buffer_size,
                    };
                    
                    // Создаём временные переменные для статического метода
                    let mut state = head.state;
                    let read_mode = head.read_mode;
                    let processor = head.processor.clone();
                    let mut pan = head.state.pan;
                    
                    Self::process_head_static(
                        0, &mut state, &read_mode, &processor, &mut pan,
                        buffer_size, sample_rate, &view, output_len, outputs
                    );
                    
                    head.state = state;
                }
            }
        }
        
        Ok(())
    }
    
    /// Безопасная, но медленная версия обработки
pub fn process_safe(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), BufferError> {
    if outputs.is_empty() {
        return Ok(());
    }
    
    if !inputs.is_empty() {
        self.write(inputs[0]);
    }
    
    let buffer_size = self.buffer_size();
    let sample_rate = self.sample_rate;
    let buffer_data = self.buffer.buffer.read();
    let output_len = outputs[0].len();
    
    let view = BufferView {
        data: &buffer_data,
        size: buffer_size,
    };
    
    // ИСПРАВЛЕНИЕ: Создаём копии состояний для обработки
    let mut head_states: Vec<(usize, HeadState, ReadMode, SampleProcessor, bool, f32)> = self.heads
        .iter()
        .enumerate()
        .map(|(idx, head)| (
            idx,
            head.state,
            head.read_mode,
            head.processor.clone(),
            head.enabled,
            head.state.pan
        ))
        .collect();
    
    // Обрабатываем каждую головку
    for (idx, mut state, read_mode, processor, enabled, mut pan) in head_states.iter_mut() {
        // ИСПРАВЛЕНИЕ: dereference the bool
        if !*enabled {
            continue;
        }
        
        // Вызываем статические методы
        Self::process_head_static(
            *idx, 
            &mut state, 
            &read_mode, 
            processor,  // processor уже &SampleProcessor
            &mut pan, 
            buffer_size, 
            sample_rate, 
            &view, 
            output_len, 
            outputs
        );
    }
    
    // Обновляем состояния в heads
    for (idx, state, _, _, _, pan) in head_states {
        if let Some(head) = self.heads.get_mut(idx) {
            head.state = state;
            head.state.pan = pan;
        }
    }
    
    Ok(())
}

}

// === Пул буферов ===

#[derive(Clone)]
pub struct BufferPool {
    buffers: Vec<AudioBuffer>,
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
    
    pub fn acquire(&mut self) -> Option<AudioBuffer> {
        self.buffers.pop()
    }
    
    pub fn release(&mut self, mut buffer: AudioBuffer) {
        if buffer.len() == self.size {
            buffer.fill(0.0);
            self.buffers.push(buffer);
        }
    }
}

// === Декораторы для обработки ===

pub struct PanningDecorator {
    pan: f32,
}

impl PanningDecorator {
    pub fn new(pan: f32) -> Self {
        Self { pan: pan.max(-1.0).min(1.0) }
    }
    
    pub fn process(&self, left: &mut [f32], right: &mut [f32]) {
        let (left_gain, right_gain) = self.pan_to_gains();
        
        for i in 0..left.len().min(right.len()) {
            left[i] *= left_gain;
            right[i] *= right_gain;
        }
    }
    
    fn pan_to_gains(&self) -> (f32, f32) {
        let pan = self.pan;
        let left_gain = if pan <= 0.0 { 1.0 } else { 1.0 - pan };
        let right_gain = if pan >= 0.0 { 1.0 } else { 1.0 + pan };
        (left_gain, right_gain)
    }
}

pub struct LfoDecorator {
    frequency: f32,
    amplitude: f32,
    phase: f32,
    sample_rate: f32,
}

impl LfoDecorator {
    pub fn new(frequency: f32, amplitude: f32, sample_rate: f32) -> Self {
        Self {
            frequency,
            amplitude: amplitude.max(0.0).min(1.0),
            phase: 0.0,
            sample_rate,
        }
    }
    
    pub fn process(&mut self, buffer: &mut [f32]) {
        let phase_increment = 2.0 * std::f32::consts::PI * self.frequency / self.sample_rate;
        
        for (i, sample) in buffer.iter_mut().enumerate() {
            let modulation = (self.phase + i as f32 * phase_increment).sin() * self.amplitude;
            *sample *= 1.0 + modulation;
        }
        
        self.phase += buffer.len() as f32 * phase_increment;
        if self.phase > 2.0 * std::f32::consts::PI {
            self.phase -= 2.0 * std::f32::consts::PI;
        }
    }
}

// === Трейт AudioNode для интеграции с kama-core ===

impl AudioNode for MultiHeadBuffer {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        self.process(inputs, outputs)
            .map_err(|e| AudioError::Processing(e.to_string()))
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "num_heads" => Some(ParamValue::Int(self.heads.len() as i32)),
            "buffer_size" => Some(ParamValue::Int(self.buffer_size() as i32)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("num_heads", ParamValue::Int(num)) => {
                let num = num.max(0).min(self.max_heads as i32) as usize;
                while self.heads.len() < num {
                    self.add_head();
                }
                while self.heads.len() > num {
                    let _ = self.remove_head(self.heads.len());
                }
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {
        for head in &mut self.heads {
            head.state.current_position = 0;
        }
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 2 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "MultiHead Buffer".to_string(),
            category: NodeCategory::Effect,
            description: "Multi-head buffer for complex playback".to_string(),
            author: "Kama Buffers".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "num_heads".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(1),
                    min: Some(0.0),
                    max: Some(self.max_heads as f32),
                    step: Some(1.0),
                    unit: Some("heads".to_string()),
                    choices: None,
                },
            ],
        }
    }
}

// === Тесты и бенчмарки ===

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    
    #[test]
    fn test_ring_buffer_basic() {
        let mut buffer = RingBuffer::new(1024);
        
        let test_data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        buffer.write(&test_data);
        
        let mut output = vec![0.0; 3];
        buffer.read(1, &mut output);
        
        assert_eq!(output[0], 5.0);
        assert_eq!(output[1], 1.0);
        assert_eq!(output[2], 2.0);
    }
    
    #[test]
    fn test_multi_head_buffer() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
        
        let head1_id = buffer.add_head();
        let head2_id = buffer.add_head();
        
        assert_eq!(head1_id, 1);
        assert_eq!(head2_id, 2);
        
        if let Some(head1) = buffer.get_head_mut(head1_id) {
            head1.state.speed = 0.5;
            head1.state.pan = -0.5;
        }
        
        if let Some(head2) = buffer.get_head_mut(head2_id) {
            head2.state.speed = 2.0;
            head2.state.pan = 0.5;
        }
        
        assert_eq!(buffer.heads.len(), 2);
    }
    
    #[test]
    fn test_buffer_pool() {
        let mut pool = BufferPool::new(4, 256);
        
        let buf1 = pool.acquire().unwrap();
        let buf2 = pool.acquire().unwrap();
        
        assert_eq!(buf1.len(), 256);
        assert_eq!(buf2.len(), 256);
        
        pool.release(buf1);
        pool.release(buf2);
        
        assert_eq!(pool.buffers.len(), 4);
    }
    
    #[test]
    fn test_process_methods() {
        let mut buffer = MultiHeadBuffer::new(4096, 44100.0);
        
        for _ in 0..4 {
            buffer.add_head();
        }
        
        let input = vec![0.5; 256];
        let mut output_left = vec![0.0; 256];
        let mut output_right = vec![0.0; 256];
        let mut outputs = [&mut output_left[..], &mut output_right[..]];
        
        // Test safe version
        let result = buffer.process_safe(&[&input], &mut outputs);
        assert!(result.is_ok());
        
        // Test optimized version
        let mut output_left2 = vec![0.0; 256];
        let mut output_right2 = vec![0.0; 256];
        let mut outputs2 = [&mut output_left2[..], &mut output_right2[..]];
        
        let result = buffer.process_optimized(&[&input], &mut outputs2);
        assert!(result.is_ok());
    }
    
    #[test]
    fn benchmark_process_methods() {
        let mut buffer = MultiHeadBuffer::new(4096, 44100.0);
        
        for _ in 0..8 {
            let head_id = buffer.add_head();
            if let Some(head) = buffer.get_head_mut(head_id) {
                head.state.speed = 0.5 + rand::thread_rng().gen::<f32>() * 1.5;
                head.state.pan = rand::thread_rng().gen::<f32>() * 2.0 - 1.0;
            }
        }
        
        let input = vec![0.5; 1024];
        let mut output_left = vec![0.0; 1024];
        let mut output_right = vec![0.0; 1024];
        let mut outputs = [&mut output_left[..], &mut output_right[..]];
        
        const ITERATIONS: usize = 1000;
        
        // Benchmark safe version
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = buffer.process_safe(&[&input], &mut outputs);
        }
        let safe_time = start.elapsed();
        
        // Clear outputs
        output_left.fill(0.0);
        output_right.fill(0.0);
        
        // Benchmark optimized version
        let start = Instant::now();
        for _ in 0..ITERATIONS {
            let _ = buffer.process_optimized(&[&input], &mut outputs);
        }
        let optimized_time = start.elapsed();
        
        println!("Safe version: {:?}", safe_time);
        println!("Optimized version: {:?}", optimized_time);
        println!("Speedup: {:.2}x", 
                safe_time.as_nanos() as f64 / optimized_time.as_nanos() as f64);
        
        // Оптимизированная версия должна быть быстрее
        assert!(optimized_time <= safe_time, "Optimized version should be faster");
    }
}