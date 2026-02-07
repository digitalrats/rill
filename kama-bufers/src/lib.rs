use std::sync::Arc;
use serde::{Serialize, Deserialize};
use parking_lot::RwLock;
use kama_core::{AudioNode, ParamValue, NodeMetadata, NodeCategory, AudioError, AudioResult};

// Re-export типов
pub use kama_core::param::{ParamValue as CoreParamValue, ParamType};

// --- Типы ошибок ---
#[derive(thiserror::Error, Debug)]
pub enum BufferError {
    #[error("Buffer configuration error: {0}")]
    Config(String),
    
    #[error("Head error: {0}")]
    Head(String),
    
    #[error("Processing error: {0}")]
    Processing(String),
    
    #[error("Audio error: {0}")]
    Audio(#[from] AudioError),
}

pub type BufferResult<T> = Result<T, BufferError>;

// --- Основные типы данных (f32 для совместимости с kama-core) ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum BufferType {
    MonoArray { size: usize },
    StereoArray { size: usize },
    MultiTrack { tracks: usize, size: usize },
    Ring { size: usize },
    Granular { size: usize, grain_size: usize },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum HeadFunction {
    Read,
    Write,
    ReadWrite,
    Erase,
    Sync,
    Granular,
    Looping,
    Reverse,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadSpec {
    pub id: usize,
    pub name: String,
    pub function: HeadFunction,
    pub delay_samples: usize,
    pub channel: usize,
    pub track: Option<usize>,
    pub params: HeadParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadParams {
    pub level: f32,
    pub pan: f32,
    pub grain_size: Option<usize>,
    pub spray: Option<f32>,
    pub feedback: f32,
    pub reverse: bool,
    pub loop_enabled: bool,
    pub loop_start: Option<usize>,
    pub loop_end: Option<usize>,
}

// --- Чистые функции обработки (f32 версия) ---

pub mod dsp {
    use super::*;
    use std::sync::Arc;
    
    // Тип для чистых функций обработки головок
    pub type HeadProcessor = fn(f32, &HeadState, &BufferView) -> f32;
    
    #[derive(Debug, Clone)]
    pub struct HeadState {
        pub spec: HeadSpec,
        pub active: bool,
        pub current_position: usize,
        pub direction: i32, // 1 = forward, -1 = reverse
        pub phase: f32,     // Для LFO и прочей модуляции
    }
    
    #[derive(Debug, Clone)]
    pub struct BufferView {
        pub data: Arc<Vec<f32>>,
        pub size: usize,
        pub channels: usize,
        pub sample_rate: f32,
    }
    
    impl BufferView {
        pub fn read(&self, position: usize, channel: usize) -> f32 {
            if channel < self.channels {
                let idx = (position % self.size) * self.channels + channel;
                self.data.get(idx).copied().unwrap_or(0.0)
            } else {
                0.0
            }
        }
        
        pub fn read_interpolated(&self, position: f32, channel: usize) -> f32 {
            let pos_floor = position.floor();
            let pos_frac = position.fract();
            
            let idx1 = (pos_floor as usize % self.size) * self.channels + channel;
            let idx2 = ((pos_floor as usize + 1) % self.size) * self.channels + channel;
            
            let sample1 = self.data.get(idx1).copied().unwrap_or(0.0);
            let sample2 = self.data.get(idx2).copied().unwrap_or(0.0);
            
            sample1 + pos_frac * (sample2 - sample1)
        }
    }
    
    // Библиотека чистых процессоров
    pub mod processors {
        use super::*;
        use std::f32::consts::PI;
        
        pub fn read_only(input: f32, head: &HeadState, buffer: &BufferView) -> f32 {
            if !head.active {
                return 0.0;
            }
            
            let mut position = head.current_position;
            
            // Обработка петли
            if head.spec.params.loop_enabled {
                if let (Some(start), Some(end)) = (head.spec.params.loop_start, head.spec.params.loop_end) {
                    if position >= end {
                        position = start + (position - end);
                    }
                }
            }
            
            let sample = buffer.read(position, head.spec.channel);
            let output = if head.spec.params.reverse {
                -sample // Простой способ имитации реверса
            } else {
                sample
            };
            
            output * head.spec.params.level
        }
        
        pub fn write_only(_input: f32, _head: &HeadState, _buffer: &BufferView) -> f32 {
            0.0 // Write head не производит output
        }
        
        pub fn read_write(input: f32, head: &HeadState, buffer: &BufferView) -> f32 {
            if !head.active {
                return 0.0;
            }
            
            // Читаем существующий sample
            let existing = buffer.read(head.current_position, head.spec.channel);
            
            // Overdub с feedback
            let mixed = existing * (1.0 - head.spec.params.feedback) + 
                       input * head.spec.params.feedback;
            
            mixed * head.spec.params.level
        }
        
        pub fn granular(input: f32, head: &HeadState, buffer: &BufferView) -> f32 {
            if !head.active {
                return 0.0;
            }
            
            if let Some(grain_size) = head.spec.params.grain_size {
                // Гранулярный синтез с windowing
                let mut sum = 0.0f32;
                let spray = head.spec.params.spray.unwrap_or(0.0);
                
                for i in 0..grain_size {
                    // Добавляем случайное смещение (spray)
                    let spray_offset = if spray > 0.0 {
                        ((rand::random::<f32>() - 0.5) * spray * grain_size as f32) as isize
                    } else {
                        0
                    };
                    
                    let pos = (head.current_position as isize + spray_offset + i as isize)
                        .max(0) as usize % buffer.size;
                    
                    // Hanning window для плавного fade
                    let window = 0.5 - 0.5 * (2.0 * PI * i as f32 / grain_size as f32).cos();
                    sum += buffer.read(pos, head.spec.channel) * window;
                }
                
                (sum / grain_size as f32) * head.spec.params.level
            } else {
                0.0
            }
        }
        
        pub fn reverse(input: f32, head: &HeadState, buffer: &BufferView) -> f32 {
            if !head.active {
                return 0.0;
            }
            
            // Читаем в обратном направлении
            let reverse_pos = buffer.size().wrapping_sub(head.current_position) % buffer.size();
            let sample = buffer.read(reverse_pos, head.spec.channel);
            
            sample * head.spec.params.level
        }
        
        // Композиция процессоров
        pub fn with_pan(processor: HeadProcessor) -> HeadProcessor {
            move |input, head, buffer| {
                let signal = processor(input, head, buffer);
                apply_pan(signal, head.spec.params.pan)
            }
        }
        
        pub fn with_lfo(processor: HeadProcessor, rate: f32, depth: f32) -> HeadProcessor {
            move |input, head, buffer| {
                let lfo = (head.phase * 2.0 * PI).sin() * depth;
                let signal = processor(input, head, buffer);
                signal * (1.0 + lfo)
            }
        }
        
        fn apply_pan(signal: f32, pan: f32) -> f32 {
            let pan = pan.clamp(-1.0, 1.0);
            // Простое панорамирование (в реальной системе нужна стерео обработка)
            signal * (1.0 - pan.abs())
        }
    }
    
    // Композитор сигнальной цепи
    pub struct SignalChain {
        processors: Vec<(usize, HeadProcessor)>,
        routing_matrix: Vec<Vec<f32>>,
        feedback_matrix: Vec<Vec<f32>>,
    }
    
    impl SignalChain {
        pub fn new() -> Self {
            Self {
                processors: Vec::new(),
                routing_matrix: Vec::new(),
                feedback_matrix: Vec::new(),
            }
        }
        
        pub fn add_processor(mut self, head_id: usize, processor: HeadProcessor) -> Self {
            self.processors.push((head_id, processor));
            self
        }
        
        pub fn route(mut self, from: usize, to: usize, amount: f32) -> Self {
            self.ensure_matrix_size(from.max(to));
            self.routing_matrix[from][to] = amount.clamp(0.0, 1.0);
            self
        }
        
        pub fn feedback(mut self, from: usize, to: usize, amount: f32) -> Self {
            self.ensure_matrix_size(from.max(to));
            self.feedback_matrix[from][to] = amount.clamp(0.0, 0.99); // Ограничение feedback
            self
        }
        
        fn ensure_matrix_size(&mut self, size: usize) {
            let target_size = size + 1;
            while self.routing_matrix.len() < target_size {
                self.routing_matrix.push(vec![0.0; target_size]);
            }
            while self.feedback_matrix.len() < target_size {
                self.feedback_matrix.push(vec![0.0; target_size]);
            }
        }
        
        pub fn process(
            &self,
            input: f32,
            heads: &[HeadState],
            buffer: &BufferView,
            feedback_buffer: &mut Vec<f32>,
        ) -> Vec<f32> {
            let mut outputs = vec![0.0; heads.len()];
            
            // Обрабатываем каждую головку
            for (head_id, processor) in &self.processors {
                if let Some(head) = heads.iter().find(|h| h.spec.id == *head_id) {
                    let processed = processor(input, head, buffer);
                    outputs[*head_id] = processed;
                }
            }
            
            // Применяем feedback
            let mut feedback_signals = vec![0.0; outputs.len()];
            for (from, row) in self.feedback_matrix.iter().enumerate() {
                if from < outputs.len() {
                    for (to, &amount) in row.iter().enumerate() {
                        if amount > 0.0 && to < feedback_signals.len() {
                            feedback_signals[to] += outputs[from] * amount;
                        }
                    }
                }
            }
            
            // Применяем routing
            let mut final_outputs = outputs.clone();
            for (from, row) in self.routing_matrix.iter().enumerate() {
                if from < outputs.len() {
                    for (to, &amount) in row.iter().enumerate() {
                        if amount > 0.0 && to < final_outputs.len() {
                            final_outputs[to] += outputs[from] * amount;
                        }
                    }
                }
            }
            
            // Сохраняем feedback для следующей итерации
            *feedback_buffer = feedback_signals;
            
            final_outputs
        }
    }
}

// --- Реализация AudioBuffer для kama-core ---

pub trait AudioBuffer: Send + Sync {
    fn write(&mut self, position: usize, channel: usize, value: f32);
    fn read(&self, position: usize, channel: usize) -> f32;
    fn read_interpolated(&self, position: f32, channel: usize) -> f32;
    fn size(&self) -> usize;
    fn channels(&self) -> usize;
    fn clear(&mut self);
    fn get_slice(&self, start: usize, end: usize, channel: usize) -> Vec<f32>;
}

#[derive(Clone)]
pub struct SharedAudioBuffer {
    data: Arc<RwLock<Vec<f32>>>,
    size: usize,
    channels: usize,
    sample_rate: f32,
}

impl SharedAudioBuffer {
    pub fn new(size: usize, channels: usize, sample_rate: f32) -> Self {
        let data = vec![0.0; size * channels];
        Self {
            data: Arc::new(RwLock::new(data)),
            size,
            channels,
            sample_rate,
        }
    }
    
    pub fn create_view(&self) -> dsp::BufferView {
        dsp::BufferView {
            data: Arc::new(self.data.read().clone()),
            size: self.size,
            channels: self.channels,
            sample_rate: self.sample_rate,
        }
    }
    
    pub fn write_batch(&mut self, start_pos: usize, samples: &[f32], channel: usize) {
        let mut data = self.data.write();
        for (i, &sample) in samples.iter().enumerate() {
            let pos = (start_pos + i) % self.size;
            let idx = pos * self.channels + channel;
            if idx < data.len() {
                data[idx] = sample;
            }
        }
    }
    
    pub fn read_batch(&self, start_pos: usize, count: usize, channel: usize) -> Vec<f32> {
        let data = self.data.read();
        let mut result = Vec::with_capacity(count);
        
        for i in 0..count {
            let pos = (start_pos + i) % self.size;
            let idx = pos * self.channels + channel;
            result.push(data.get(idx).copied().unwrap_or(0.0));
        }
        
        result
    }
}

impl AudioBuffer for SharedAudioBuffer {
    fn write(&mut self, position: usize, channel: usize, value: f32) {
        let mut data = self.data.write();
        let idx = (position % self.size) * self.channels + channel;
        if idx < data.len() {
            data[idx] = value;
        }
    }
    
    fn read(&self, position: usize, channel: usize) -> f32 {
        let data = self.data.read();
        let idx = (position % self.size) * self.channels + channel;
        data.get(idx).copied().unwrap_or(0.0)
    }
    
    fn read_interpolated(&self, position: f32, channel: usize) -> f32 {
        let pos_floor = position.floor();
        let pos_frac = position.fract();
        
        let data = self.data.read();
        let idx1 = (pos_floor as usize % self.size) * self.channels + channel;
        let idx2 = ((pos_floor as usize + 1) % self.size) * self.channels + channel;
        
        let sample1 = data.get(idx1).copied().unwrap_or(0.0);
        let sample2 = data.get(idx2).copied().unwrap_or(0.0);
        
        sample1 + pos_frac * (sample2 - sample1)
    }
    
    fn size(&self) -> usize { self.size }
    fn channels(&self) -> usize { self.channels }
    
    fn clear(&mut self) {
        let mut data = self.data.write();
        data.fill(0.0);
    }
    
    fn get_slice(&self, start: usize, end: usize, channel: usize) -> Vec<f32> {
        let data = self.data.read();
        let mut result = Vec::new();
        
        for i in start..end {
            let pos = i % self.size;
            let idx = pos * self.channels + channel;
            result.push(data.get(idx).copied().unwrap_or(0.0));
        }
        
        result
    }
}

// --- AudioNode реализация универсальной системы буферов ---

pub struct UniversalBufferSystem {
    config: BufferSystemConfig,
    buffer: SharedAudioBuffer,
    heads: Vec<dsp::HeadState>,
    signal_chain: dsp::SignalChain,
    write_position: usize,
    sample_rate: f32,
    feedback_buffer: Vec<f32>,
    temp_buffers: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferSystemConfig {
    pub name: String,
    pub buffer_type: BufferType,
    pub heads: Vec<HeadSpec>,
    pub routing: Vec<RouteConfig>,
    pub feedback: Vec<FeedbackConfig>,
    pub sample_rate: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    pub from: usize,
    pub to: usize,
    pub amount: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackConfig {
    pub from: usize,
    pub to: usize,
    pub amount: f32,
}

impl UniversalBufferSystem {
    pub fn new(config: BufferSystemConfig) -> BufferResult<Self> {
        let (size, channels) = match config.buffer_type {
            BufferType::MonoArray { size } => (size, 1),
            BufferType::StereoArray { size } => (size, 2),
            BufferType::MultiTrack { tracks, size } => (size, tracks * 2),
            BufferType::Ring { size } => (size, 1),
            BufferType::Granular { size, .. } => (size, 1),
        };
        
        let buffer = SharedAudioBuffer::new(size, channels, config.sample_rate);
        
        // Инициализируем головки
        let heads = config.heads.iter()
            .map(|spec| dsp::HeadState {
                spec: spec.clone(),
                active: true,
                current_position: 0,
                direction: 1,
                phase: 0.0,
            })
            .collect();
        
        // Строим signal chain
        let mut signal_chain = dsp::SignalChain::new();
        
        // Добавляем процессоры для каждой головки
        for head in &heads {
            let processor = match head.spec.function {
                HeadFunction::Read => dsp::processors::read_only,
                HeadFunction::Write => dsp::processors::write_only,
                HeadFunction::ReadWrite => dsp::processors::read_write,
                HeadFunction::Granular => dsp::processors::granular,
                HeadFunction::Reverse => dsp::processors::reverse,
                _ => dsp::processors::read_only,
            };
            
            signal_chain = signal_chain.add_processor(head.spec.id, processor);
        }
        
        // Добавляем routing
        for route in &config.routing {
            signal_chain = signal_chain.route(route.from, route.to, route.amount);
        }
        
        // Добавляем feedback
        for feedback in &config.feedback {
            signal_chain = signal_chain.feedback(feedback.from, feedback.to, feedback.amount);
        }
        
        Ok(Self {
            config,
            buffer,
            heads,
            signal_chain,
            write_position: 0,
            sample_rate: config.sample_rate,
            feedback_buffer: Vec::new(),
            temp_buffers: Vec::new(),
        })
    }
    
    fn update_head_positions(&mut self) {
        for head in &mut self.heads {
            if !head.active {
                continue;
            }
            
            // Обновляем фазу для LFO и прочей модуляции
            head.phase = (head.phase + 0.01) % 1.0;
            
            // Рассчитываем позицию с учетом задержки и направления
            let delay_samples = head.spec.delay_samples;
            let direction = if head.spec.params.reverse { -1 } else { 1 };
            
            head.current_position = match direction {
                1 => {
                    // Вперед
                    if delay_samples <= self.write_position {
                        self.write_position - delay_samples
                    } else {
                        self.buffer.size() - (delay_samples - self.write_position)
                    }
                }
                -1 => {
                    // Назад
                    (self.write_position + delay_samples) % self.buffer.size()
                }
                _ => self.write_position,
            } % self.buffer.size();
            
            // Обработка петли
            if head.spec.params.loop_enabled {
                if let (Some(start), Some(end)) = (head.spec.params.loop_start, head.spec.params.loop_end) {
                    if head.current_position >= end {
                        head.current_position = start + (head.current_position - end);
                    } else if head.current_position < start {
                        head.current_position = end - (start - head.current_position);
                    }
                }
            }
        }
    }
    
    fn process_write_heads(&mut self, input: f32, outputs: &[f32]) {
        for head in &self.heads {
            if !head.active {
                continue;
            }
            
            // Только головки записи пишут в буфер
            if matches!(head.spec.function, HeadFunction::Write | HeadFunction::ReadWrite) {
                let head_output = outputs.get(head.spec.id).copied().unwrap_or(0.0);
                
                let write_value = match head.spec.function {
                    HeadFunction::Write => input * head.spec.params.level,
                    HeadFunction::ReadWrite => {
                        // Overdub с существующим содержимым
                        let existing = self.buffer.read(head.current_position, head.spec.channel);
                        existing * (1.0 - head.spec.params.feedback) + 
                        input * head.spec.params.feedback
                    }
                    _ => 0.0,
                };
                
                self.buffer.write(head.current_position, head.spec.channel, write_value + head_output);
            }
        }
    }
    
    pub fn set_head_level(&mut self, head_id: usize, level: f32) {
        if let Some(head) = self.heads.iter_mut().find(|h| h.spec.id == head_id) {
            head.spec.params.level = level.clamp(0.0, 1.0);
        }
    }
    
    pub fn set_head_pan(&mut self, head_id: usize, pan: f32) {
        if let Some(head) = self.heads.iter_mut().find(|h| h.spec.id == head_id) {
            head.spec.params.pan = pan.clamp(-1.0, 1.0);
        }
    }
    
    pub fn set_head_feedback(&mut self, head_id: usize, feedback: f32) {
        if let Some(head) = self.heads.iter_mut().find(|h| h.spec.id == head_id) {
            head.spec.params.feedback = feedback.clamp(0.0, 0.99);
        }
    }
    
    pub fn toggle_head_active(&mut self, head_id: usize) -> bool {
        if let Some(head) = self.heads.iter_mut().find(|h| h.spec.id == head_id) {
            head.active = !head.active;
            head.active
        } else {
            false
        }
    }
    
    pub fn set_loop(&mut self, head_id: usize, start: usize, end: usize) {
        if let Some(head) = self.heads.iter_mut().find(|h| h.spec.id == head_id) {
            head.spec.params.loop_enabled = true;
            head.spec.params.loop_start = Some(start);
            head.spec.params.loop_end = Some(end);
        }
    }
    
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
    }
    
    pub fn get_buffer_snapshot(&self, channel: usize) -> Vec<f32> {
        self.buffer.get_slice(0, self.buffer.size(), channel)
    }
    
    pub fn export_config(&self) -> BufferSystemConfig {
        self.config.clone()
    }
}

impl AudioNode for UniversalBufferSystem {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let buffer_size = outputs[0].len();
        
        // Подготавливаем временные буферы
        if self.temp_buffers.len() < self.heads.len() {
            self.temp_buffers = vec![vec![0.0; buffer_size]; self.heads.len()];
        }
        
        // Обрабатываем каждый sample
        for sample_idx in 0..buffer_size {
            // Получаем входной sample (берём первый вход)
            let input_sample = inputs[0].get(sample_idx).copied().unwrap_or(0.0);
            
            // Обновляем позиции головок
            self.update_head_positions();
            
            // Получаем view буфера
            let buffer_view = self.buffer.create_view();
            
            // Обрабатываем через signal chain
            let head_outputs = self.signal_chain.process(
                input_sample,
                &self.heads,
                &buffer_view,
                &mut self.feedback_buffer,
            );
            
            // Пишем через головки записи
            self.process_write_heads(input_sample, &head_outputs);
            
            // Сохраняем выходы во временные буферы
            for (head_id, &output) in head_outputs.iter().enumerate() {
                if head_id < self.temp_buffers.len() {
                    self.temp_buffers[head_id][sample_idx] = output;
                }
            }
            
            // Обновляем позицию записи
            self.write_position = (self.write_position + 1) % self.buffer.size();
        }
        
        // Копируем выходы в выходные буферы
        for (out_idx, output) in outputs.iter_mut().enumerate() {
            if out_idx < self.temp_buffers.len() {
                for (i, sample) in self.temp_buffers[out_idx].iter().enumerate() {
                    if i < output.len() {
                        output[i] = *sample;
                    }
                }
            }
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "buffer_size" => Some(ParamValue::Int(self.buffer.size() as i32)),
            "channels" => Some(ParamValue::Int(self.buffer.channels() as i32)),
            _ => {
                // Пытаемся получить параметр головки
                if let Some((head_id_str, param_name)) = name.split_once('_') {
                    if let Ok(head_id) = head_id_str.parse::<usize>() {
                        if let Some(head) = self.heads.iter().find(|h| h.spec.id == head_id) {
                            return match param_name {
                                "level" => Some(ParamValue::Float(head.spec.params.level)),
                                "pan" => Some(ParamValue::Float(head.spec.params.pan)),
                                "feedback" => Some(ParamValue::Float(head.spec.params.feedback)),
                                "active" => Some(ParamValue::Bool(head.active)),
                                "delay" => Some(ParamValue::Int(head.spec.delay_samples as i32)),
                                _ => None,
                            };
                        }
                    }
                }
                None
            }
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            // Глобальные параметры
            ("clear_buffer", ParamValue::Bool(true)) => {
                self.clear_buffer();
                Ok(())
            }
            _ => {
                // Параметры головок
                if let Some((head_id_str, param_name)) = name.split_once('_') {
                    if let Ok(head_id) = head_id_str.parse::<usize>() {
                        return match (param_name, value) {
                            ("level", ParamValue::Float(v)) => {
                                self.set_head_level(head_id, v);
                                Ok(())
                            }
                            ("pan", ParamValue::Float(v)) => {
                                self.set_head_pan(head_id, v);
                                Ok(())
                            }
                            ("feedback", ParamValue::Float(v)) => {
                                self.set_head_feedback(head_id, v);
                                Ok(())
                            }
                            ("active", ParamValue::Bool(v)) => {
                                if let Some(head) = self.heads.iter_mut().find(|h| h.spec.id == head_id) {
                                    head.active = v;
                                }
                                Ok(())
                            }
                            ("delay", ParamValue::Int(v)) => {
                                if let Some(head) = self.heads.iter_mut().find(|h| h.spec.id == head_id) {
                                    head.spec.delay_samples = v as usize;
                                }
                                Ok(())
                            }
                            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
                        };
                    }
                }
                Err(AudioError::Parameter(format!("Unknown parameter: {}", name)))
            }
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.config.sample_rate = sample_rate;
        // Можно пересоздать буфер с новым sample rate если нужно
    }
    
    fn reset(&mut self) {
        self.clear_buffer();
        self.write_position = 0;
        self.feedback_buffer.clear();
        
        for head in &mut self.heads {
            head.current_position = 0;
            head.phase = 0.0;
            head.active = true;
        }
    }
    
    fn num_inputs(&self) -> usize {
        1 // Универсальная система обычно имеет один вход
    }
    
    fn num_outputs(&self) -> usize {
        self.heads.len()
    }
    
    fn metadata(&self) -> NodeMetadata {
        let mut params = Vec::new();
        
        // Добавляем параметры каждой головки
        for head in &self.heads {
            params.extend(vec![
                kama_core::node::ParamMetadata {
                    name: format!("{}_level", head.spec.id),
                    typ: ParamType::Float,
                    default: ParamValue::Float(head.spec.params.level),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("linear".to_string()),
                    choices: None,
                },
                kama_core::node::ParamMetadata {
                    name: format!("{}_pan", head.spec.id),
                    typ: ParamType::Float,
                    default: ParamValue::Float(head.spec.params.pan),
                    min: Some(-1.0),
                    max: Some(1.0),
                    step: Some(0.01),
                    unit: Some("pan".to_string()),
                    choices: None,
                },
                kama_core::node::ParamMetadata {
                    name: format!("{}_feedback", head.spec.id),
                    typ: ParamType::Float,
                    default: ParamValue::Float(head.spec.params.feedback),
                    min: Some(0.0),
                    max: Some(0.99),
                    step: Some(0.01),
                    unit: Some("linear".to_string()),
                    choices: None,
                },
                kama_core::node::ParamMetadata {
                    name: format!("{}_active", head.spec.id),
                    typ: ParamType::Bool,
                    default: ParamValue::Bool(head.active),
                    min: None,
                    max: None,
                    step: None,
                    unit: None,
                    choices: None,
                },
            ]);
        }
        
        // Глобальные параметры
        params.push(kama_core::node::ParamMetadata {
            name: "clear_buffer".to_string(),
            typ: ParamType::Bool,
            default: ParamValue::Bool(false),
            min: None,
            max: None,
            step: None,
            unit: None,
            choices: None,
        });
        
        NodeMetadata {
            name: self.config.name.clone(),
            category: NodeCategory::Utility,
            description: "Universal buffer system for tape emulation, sampling, and granular synthesis".to_string(),
            author: "Kama Buffers".to_string(),
            version: "1.0".to_string(),
            parameters: params,
        }
    }
}

// --- Фабрики предустановок ---

pub struct BufferPresetFactory;

impl BufferPresetFactory {
    pub fn create_tape_delay(sample_rate: f32) -> UniversalBufferSystem {
        let config = BufferSystemConfig {
            name: "Tape Delay".to_string(),
            buffer_type: BufferType::MonoArray { size: (sample_rate * 2.0) as usize }, // 2 секунды
            heads: vec![
                HeadSpec {
                    id: 0,
                    name: "Write Head".to_string(),
                    function: HeadFunction::Write,
                    delay_samples: 0,
                    channel: 0,
                    track: None,
                    params: HeadParams {
                        level: 1.0,
                        pan: 0.0,
                        grain_size: None,
                        spray: None,
                        feedback: 0.0,
                        reverse: false,
                        loop_enabled: false,
                        loop_start: None,
                        loop_end: None,
                    },
                },
                HeadSpec {
                    id: 1,
                    name: "Read Head 1".to_string(),
                    function: HeadFunction::Read,
                    delay_samples: (sample_rate * 0.3) as usize, // 300ms задержка
                    channel: 0,
                    track: None,
                    params: HeadParams {
                        level: 0.8,
                        pan: -0.5,
                        grain_size: None,
                        spray: None,
                        feedback: 0.0,
                        reverse: false,
                        loop_enabled: false,
                        loop_start: None,
                        loop_end: None,
                    },
                },
                HeadSpec {
                    id: 2,
                    name: "Read Head 2".to_string(),
                    function: HeadFunction::Read,
                    delay_samples: (sample_rate * 0.5) as usize, // 500ms задержка
                    channel: 0,
                    track: None,
                    params: HeadParams {
                        level: 0.6,
                        pan: 0.5,
                        grain_size: None,
                        spray: None,
                        feedback: 0.0,
                        reverse: false,
                        loop_enabled: false,
                        loop_start: None,
                        loop_end: None,
                    },
                },
            ],
            routing: vec![
                RouteConfig { from: 1, to: 0, amount: 0.6 }, // Feedback к write head
                RouteConfig { from: 2, to: 0, amount: 0.4 },
            ],
            feedback: vec![
                FeedbackConfig { from: 1, to: 0, amount: 0.6 },
            ],
            sample_rate,
        };
        
        UniversalBufferSystem::new(config).expect("Failed to create tape delay")
    }
    
    pub fn create_granular_sampler(sample_rate: f32) -> UniversalBufferSystem {
        let config = BufferSystemConfig {
            name: "Granular Sampler".to_string(),
            buffer_type: BufferType::Granular { 
                size: (sample_rate * 5.0) as usize, // 5 секунд
                grain_size: 2048,
            },
            heads: vec![
                HeadSpec {
                    id: 0,
                    name: "Record Head".to_string(),
                    function: HeadFunction::Write,
                    delay_samples: 0,
                    channel: 0,
                    track: None,
                    params: HeadParams {
                        level: 1.0,
                        pan: 0.0,
                        grain_size: None,
                        spray: None,
                        feedback: 0.0,
                        reverse: false,
                        loop_enabled: false,
                        loop_start: None,
                        loop_end: None,
                    },
                },
                HeadSpec {
                    id: 1,
                    name: "Granular Head 1".to_string(),
                    function: HeadFunction::Granular,
                    delay_samples: (sample_rate * 0.5) as usize,
                    channel: 0,
                    track: None,
                    params: HeadParams {
                        level: 0.7,
                        pan: -0.3,
                        grain_size: Some(1024),
                        spray: Some(0.1),
                        feedback: 0.0,
                        reverse: false,
                        loop_enabled: true,
                        loop_start: Some((sample_rate * 1.0) as usize),
                        loop_end: Some((sample_rate * 3.0) as usize),
                    },
                },
                HeadSpec {
                    id: 2,
                    name: "Granular Head 2".to_string(),
                    function: HeadFunction::Granular,
                    delay_samples: (sample_rate * 0.7) as usize,
                    channel: 0,
                    track: None,
                    params: HeadParams {
                        level: 0.5,
                        pan: 0.3,
                        grain_size: Some(512),
                        spray: Some(0.2),
                        feedback: 0.0,
                        reverse: true, // Реверс гранул
                        loop_enabled: true,
                        loop_start: Some((sample_rate * 1.0) as usize),
                        loop_end: Some((sample_rate * 3.0) as usize),
                    },
                },
            ],
            routing: Vec::new(),
            feedback: Vec::new(),
            sample_rate,
        };
        
        UniversalBufferSystem::new(config).expect("Failed to create granular sampler")
    }
    
    pub fn create_multi_track_recorder(sample_rate: f32, tracks: usize) -> UniversalBufferSystem {
        let mut heads = Vec::new();
        
        for track in 0..tracks {
            heads.push(HeadSpec {
                id: track * 2,
                name: format!("Track {} L", track + 1),
                function: HeadFunction::ReadWrite,
                delay_samples: 0,
                channel: track * 2,
                track: Some(track),
                params: HeadParams {
                    level: 1.0,
                    pan: if track % 2 == 0 { -0.3 } else { 0.0 },
                    grain_size: None,
                    spray: None,
                    feedback: 0.0,
                    reverse: false,
                    loop_enabled: true,
                    loop_start: Some(0),
                    loop_end: Some((sample_rate * 4.0) as usize),
                },
            });
            
            heads.push(HeadSpec {
                id: track * 2 + 1,
                name: format!("Track {} R", track + 1),
                function: HeadFunction::ReadWrite,
                delay_samples: 0,
                channel: track * 2 + 1,
                track: Some(track),
                params: HeadParams {
                    level: 1.0,
                    pan: if track % 2 == 0 { 0.0 } else { 0.3 },
                    grain_size: None,
                    spray: None,
                    feedback: 0.0,
                    reverse: false,
                    loop_enabled: true,
                    loop_start: Some(0),
                    loop_end: Some((sample_rate * 4.0) as usize),
                },
            });
        }
        
        let config = BufferSystemConfig {
            name: format!("{} Track Recorder", tracks),
            buffer_type: BufferType::MultiTrack { 
                tracks,
                size: (sample_rate * 10.0) as usize, // 10 секунд
            },
            heads,
            routing: Vec::new(),
            feedback: Vec::new(),
            sample_rate,
        };
        
        UniversalBufferSystem::new(config).expect("Failed to create multi-track recorder")
    }
}

// --- Утилиты для работы с буферами ---

pub mod utils {
    use super::*;
    
    /// Копирует данные из одного буфера в другой
    pub fn copy_buffer(
        src: &dyn AudioBuffer,
        src_start: usize,
        dest: &mut dyn AudioBuffer,
        dest_start: usize,
        length: usize,
        src_channel: usize,
        dest_channel: usize,
    ) {
        for i in 0..length {
            let sample = src.read((src_start + i) % src.size(), src_channel);
            dest.write((dest_start + i) % dest.size(), dest_channel, sample);
        }
    }
    
    /// Применяет функцию к каждому sample в буфере
    pub fn map_buffer<F>(
        buffer: &mut dyn AudioBuffer,
        channel: usize,
        mapper: F,
    ) where
        F: Fn(f32) -> f32,
    {
        for i in 0..buffer.size() {
            let sample = buffer.read(i, channel);
            buffer.write(i, channel, mapper(sample));
        }
    }
    
    /// Нормализует буфер
    pub fn normalize_buffer(
        buffer: &mut dyn AudioBuffer,
        channel: usize,
        target_peak: f32,
    ) {
        let mut max_amplitude = 0.0f32;
        
        // Находим пиковую амплитуду
        for i in 0..buffer.size() {
            let sample = buffer.read(i, channel).abs();
            if sample > max_amplitude {
                max_amplitude = sample;
            }
        }
        
        if max_amplitude > 0.0 {
            let gain = target_peak / max_amplitude;
            map_buffer(buffer, channel, |x| x * gain);
        }
    }
    
    /// Создает синусоидальный тестовый сигнал
    pub fn generate_test_signal(
        buffer: &mut dyn AudioBuffer,
        channel: usize,
        frequency: f32,
        sample_rate: f32,
        amplitude: f32,
    ) {
        for i in 0..buffer.size() {
            let time = i as f32 / sample_rate;
            let sample = (2.0 * std::f32::consts::PI * frequency * time).sin() * amplitude;
            buffer.write(i, channel, sample);
        }
    }
}

// --- Примеры использования ---

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_tape_delay_creation() {
        let delay = BufferPresetFactory::create_tape_delay(44100.0);
        
        assert_eq!(delay.num_inputs(), 1);
        assert_eq!(delay.num_outputs(), 3); // Write head + 2 read heads
        
        let metadata = delay.metadata();
        assert_eq!(metadata.name, "Tape Delay");
        assert_eq!(metadata.category, NodeCategory::Utility);
    }
    
    #[test]
    fn test_buffer_operations() {
        let mut buffer = SharedAudioBuffer::new(1024, 2, 44100.0);
        
        // Тест записи и чтения
        buffer.write(0, 0, 0.5);
        assert_eq!(buffer.read(0, 0), 0.5);
        
        // Тест интерполяции
        buffer.write(0, 0, 0.0);
        buffer.write(1, 0, 1.0);
        let interpolated = buffer.read_interpolated(0.5, 0);
        assert!(interpolated > 0.0 && interpolated < 1.0);
        
        // Тест batch операций
        let test_samples = vec![0.1, 0.2, 0.3, 0.4];
        buffer.write_batch(0, &test_samples, 1);
        
        let read_samples = buffer.read_batch(0, 4, 1);
        assert_eq!(read_samples, test_samples);
    }
    
    #[test]
    fn test_granular_sampler() {
        let mut sampler = BufferPresetFactory::create_granular_sampler(44100.0);
        
        // Записываем тестовый сигнал
        let mut input_buffer = vec![0.0f32; 1024];
        let mut output_buffers = vec![vec![0.0f32; 1024]; sampler.num_outputs()];
        
        let input_refs = [&input_buffer[..]];
        let mut output_refs: Vec<&mut [f32]> = output_buffers.iter_mut()
            .map(|buf| &mut buf[..])
            .collect();
        
        // Обрабатываем
        sampler.process(&input_refs, &mut output_refs).unwrap();
        
        // Проверяем, что есть выходной сигнал
        assert!(output_buffers[1].iter().any(|&x| x != 0.0));
    }
    
    #[test]
    fn test_multi_track_recorder() {
        let recorder = BufferPresetFactory::create_multi_track_recorder(44100.0, 4);
        
        assert_eq!(recorder.num_outputs(), 8); // 4 стерео трека = 8 каналов
        
        // Проверяем параметры головок
        for head_id in 0..8 {
            let param_name = format!("{}_active", head_id);
            assert!(recorder.get_param(&param_name).is_some());
        }
    }
    
    #[test]
    fn test_utils_functions() {
        let mut buffer = SharedAudioBuffer::new(256, 1, 44100.0);
        
        // Генерируем тестовый сигнал
        utils::generate_test_signal(&mut buffer, 0, 440.0, 44100.0, 0.5);
        
        // Проверяем, что сигнал сгенерирован
        let sample = buffer.read(0, 0);
        assert_ne!(sample, 0.0);
        
        // Тест нормализации
        utils::normalize_buffer(&mut buffer, 0, 1.0);
        
        let max_sample = (0..buffer.size())
            .map(|i| buffer.read(i, 0).abs())
            .fold(0.0f32, |a, b| a.max(b));
        
        assert!(max_sample <= 1.0);
    }
}