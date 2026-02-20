use rand::Rng;

use kama_core_traits::{
    AudioNode, 
    AudioError,
    param::{ParamValue, ParamType, ParamMetadata},
    node::{NodeMetadata, NodeCategory, NodeTypeId},
};

use crate::{
    ring::RingBuffer,
    head::{BufferHead, HeadState, ReadMode},
    view::BufferView,
    processor::SampleProcessor,
    error::{BufferError, BufferResult},
};

/// Многоголовый буфер для сложного воспроизведения
#[derive(Clone)]
pub struct MultiHeadBuffer {
    buffer: RingBuffer,
    heads: Vec<BufferHead>,
    sample_rate: f32,
    max_heads: usize,
}

impl MultiHeadBuffer {
    /// Создать новый многоголовый буфер
    pub fn new(size: usize, sample_rate: f32) -> Self {
        Self {
            buffer: RingBuffer::new(size),
            heads: Vec::new(),
            sample_rate,
            max_heads: 8,
        }
    }
    
    /// Добавить новую головку воспроизведения
    pub fn add_head(&mut self) -> usize {
        if self.heads.len() >= self.max_heads {
            return 0;
        }
        
        let id = self.heads.len() + 1;
        let head = BufferHead::new(id);
        self.heads.push(head);
        id
    }
    
    /// Удалить головку по ID
    pub fn remove_head(&mut self, id: usize) -> BufferResult<()> {
        if id == 0 || id > self.heads.len() {
            return Err(BufferError::InvalidHeadId(id));
        }
        
        self.heads.remove(id - 1);
        // Перенумеровываем оставшиеся головки
        for (i, head) in self.heads.iter_mut().enumerate() {
            head.id = i + 1;
        }
        
        Ok(())
    }
    
    /// Получить ссылку на головку
    pub fn get_head(&self, id: usize) -> Option<&BufferHead> {
        if id == 0 || id > self.heads.len() {
            return None;
        }
        self.heads.get(id - 1)
    }
    
    /// Получить мутабельную ссылку на головку
    pub fn get_head_mut(&mut self, id: usize) -> Option<&mut BufferHead> {
        if id == 0 || id > self.heads.len() {
            return None;
        }
        self.heads.get_mut(id - 1)
    }
    
    /// Записать семплы в буфер
    pub fn write(&mut self, samples: &[f32]) {
        self.buffer.write(samples);
    }
    
    /// Получить размер буфера
    pub fn buffer_size(&self) -> usize {
        self.buffer.size()
    }
    
    /// Сбросить состояние всех головок
    pub fn reset(&mut self) {
        for head in &mut self.heads {
            head.state.current_position = 0;
        }
    }
    
    /// Основной метод обработки (выбирает оптимальную версию)
    pub fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), BufferError> {
        self.process_optimized(inputs, outputs)
    }
    
/// Оптимизированная версия для производительности (без unsafe)
pub fn process_optimized(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), BufferError> {
    if outputs.is_empty() {
        return Ok(());
    }
    
    if !inputs.is_empty() {
        self.write(inputs[0]);
    }
    
    let buffer_size = self.buffer_size();
    let buffer_mask = buffer_size - 1;
    let sample_rate = self.sample_rate;
    let output_len = outputs[0].len();
    
    // Получаем безопасный доступ к данным буфера
    let buffer_guard = self.buffer.read_guard();
    let buffer_slice = &buffer_guard[..];
    
    for head in &mut self.heads {
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
                // Используем split_at_mut для получения двух мутабельных срезов
                let (left, right) = outputs.split_at_mut(1);
                let left = &mut left[0];
                let right = &mut right[0];
                
                // Убеждаемся, что у нас достаточно места
                let process_len = output_len.min(left.len()).min(right.len());
                
                for i in 0..process_len {
                    // Вычисляем позицию чтения с интерполяцией
                    let pos_f = (current_pos as f32 * speed) as f32;
                    let pos_floor = pos_f.floor() as usize;
                    let frac = pos_f.fract();
                    
                    // Индексы с учетом маски и размера буфера
                    let idx1 = (pos_floor & buffer_mask) % buffer_size;
                    let idx2 = ((pos_floor + 1) & buffer_mask) % buffer_size;
                    
                    // Безопасное чтение из среза
                    let s1 = buffer_slice[idx1];
                    let s2 = buffer_slice[idx2];
                    
                    // Линейная интерполяция
                    let sample = s1 + frac * (s2 - s1);
                    let processed = sample * volume;
                    
                    // Запись с панорамированием
                    left[i] += processed * left_gain;
                    right[i] += processed * right_gain;
                    
                    // Обновляем позицию
                    current_pos = (current_pos + 1) % buffer_size;
                }
            } else {
                // Моно выход
                let mono = &mut outputs[0];
                let process_len = output_len.min(mono.len());
                
                for i in 0..process_len {
                    // Вычисляем позицию чтения с интерполяцией
                    let pos_f = (current_pos as f32 * speed) as f32;
                    let pos_floor = pos_f.floor() as usize;
                    let frac = pos_f.fract();
                    
                    // Индексы с учетом маски и размера буфера
                    let idx1 = (pos_floor & buffer_mask) % buffer_size;
                    let idx2 = ((pos_floor + 1) & buffer_mask) % buffer_size;
                    
                    // Безопасное чтение из среза
                    let s1 = buffer_slice[idx1];
                    let s2 = buffer_slice[idx2];
                    
                    // Линейная интерполяция
                    let sample = s1 + frac * (s2 - s1);
                    let processed = sample * volume;
                    
                    // Запись
                    mono[i] += processed;
                    
                    // Обновляем позицию
                    current_pos = (current_pos + 1) % buffer_size;
                }
            }
            
            // Сохраняем обновленную позицию
            head.state.current_position = current_pos;
        } else {
            // Для сложных режимов используем версию с view
            let view = BufferView::new(buffer_slice, buffer_size);
            Self::process_head_complex(head, &view, sample_rate, output_len, outputs);
        }
    }
    
    Ok(())
}
    
    /// Безопасная, но более медленная версия обработки
    pub fn process_safe(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), BufferError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        if !inputs.is_empty() {
            self.write(inputs[0]);
        }
        
        let buffer_size = self.buffer_size();
        let sample_rate = self.sample_rate;
        let buffer_guard = self.buffer.read_guard();
        let buffer_slice = &buffer_guard[..];
        let output_len = outputs[0].len();
        
        let view = BufferView::new(buffer_slice, buffer_size);
        
        for head in &mut self.heads {
            if !head.enabled {
                continue;
            }
            
            Self::process_head_complex(head, &view, sample_rate, output_len, outputs);
        }
        
        Ok(())
    }
    
    /// Обработка с предварительно выделенным хранилищем
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
        
        let mut outputs: Vec<&mut [f32]> = channel_slices;
        self.process_optimized(inputs, &mut outputs)
    }
    
/// Обработка сложных режимов (Granular, Reverse, PingPong)
fn process_head_complex(
    head: &mut BufferHead,
    view: &BufferView,
    sample_rate: f32,
    output_len: usize,
    outputs: &mut [&mut [f32]]
) {
    // Если есть стерео выходы, разделяем их для записи
    let (left_out, right_out) = if outputs.len() >= 2 {
        let (left, right) = outputs.split_at_mut(1);
        (Some(&mut left[0][..]), Some(&mut right[0][..]))
    } else {
        (Some(&mut outputs[0][..]), None)
    };
    
    // Преобразуем Option<&mut [f32]> в Option<&mut [f32]> для использования в цикле
    // и сохраняем как изменяемые ссылки
    let mut left_slice = left_out;
    let mut right_slice = right_out;
    
    match head.read_mode {
        ReadMode::Simple => {
            // Уже обработано в оптимизированной ветке
        }
        ReadMode::Granular { grain_size, grain_spacing: _, randomization } => {
            let mut grain_pos = 0;
            let mut in_grain = false;
            let mut rng = rand::thread_rng();
            
            for i in 0..output_len {
                if !in_grain || grain_pos >= grain_size {
                    // Начинаем новый гран
                    let random_offset = if randomization > 0.0 {
                        (rng.gen::<f32>() * 2.0 - 1.0) * randomization * view.size() as f32
                    } else {
                        0.0
                    };
                    
                    head.state.current_position = ((head.state.current_position as f32 + random_offset) as usize) % view.size();
                    grain_pos = 0;
                    in_grain = true;
                }
                
                if grain_pos < grain_size {
                    // Применяем оконную функцию
                    let window = Self::grain_window(grain_pos, grain_size);
                    let sample = Self::read_sample(&head.state, view);
                    let processed = Self::process_sample(sample, &head.state, &head.processor, sample_rate, view) * window;
                    
                    // Запись с панорамированием
                    let (left_gain, right_gain) = Self::pan_to_gains(head.state.pan);
                    
                    if let Some(left) = left_slice.as_mut() {
                        if i < left.len() {
                            left[i] += processed * left_gain;
                        }
                    }
                    if let Some(right) = right_slice.as_mut() {
                        if i < right.len() {
                            right[i] += processed * right_gain;
                        }
                    }
                    
                    grain_pos += 1;
                } else {
                    // Тишина между гранами
                    grain_pos += 1;
                }
                
                head.state.current_position = (head.state.current_position + 1) % view.size();
            }
        }
        ReadMode::Reverse => {
            for i in 0..output_len {
                // В reverse режиме мы читаем от конца к началу
                // Вычисляем позицию: от конца буфера назад
                let read_pos = (head.state.current_position as i32 - 1).rem_euclid(view.size() as i32) as usize;
                let sample = view.get(read_pos);
                let processed = Self::process_sample(sample, &head.state, &head.processor, sample_rate, view);
                
                Self::write_sample(processed, head.state.pan, i, outputs);
                
                // Увеличиваем позицию (но читаем с конца)
                head.state.current_position = (head.state.current_position + 1) % view.size();
            }
        }
        ReadMode::PingPong { segment_size } => {
            let mut direction_forward = true;
            let mut segment_pos = 0;
            
            for i in 0..output_len {
                let sample = Self::read_sample(&head.state, view);
                let processed = Self::process_sample(sample, &head.state, &head.processor, sample_rate, view);
                
                let (left_gain, right_gain) = Self::pan_to_gains(head.state.pan);
                
                if let Some(left) = left_slice.as_mut() {
                    if i < left.len() {
                        left[i] += processed * left_gain;
                    }
                }
                if let Some(right) = right_slice.as_mut() {
                    if i < right.len() {
                        right[i] += processed * right_gain;
                    }
                }
                
                segment_pos += 1;
                if segment_pos >= segment_size {
                    direction_forward = !direction_forward;
                    segment_pos = 0;
                }
                
                if direction_forward {
                    head.state.current_position = (head.state.current_position + 1) % view.size();
                } else {
                    head.state.current_position = (head.state.current_position + view.size() - 1) % view.size();
                }
            }
        }
    }
}

fn read_sample_with_speed(state: &HeadState, view: &BufferView, speed: f32) -> f32 {
    let pos = state.current_position as f32 * speed;
    // Нормализуем позицию для обратного воспроизведения
    let normalized_pos = if pos < 0.0 {
        view.size() as f32 + pos
    } else {
        pos
    };
    view.get_interpolated(normalized_pos)
}

/// Вспомогательный метод для преобразования панорамы в коэффициенты
fn pan_to_gains(pan: f32) -> (f32, f32) {
    if pan <= 0.0 {
        (1.0, 1.0 + pan)
    } else {
        (1.0 - pan, 1.0)
    }
}
    
    // Вспомогательные методы для обработки
    
    fn read_sample(state: &HeadState, view: &BufferView) -> f32 {
        let pos = state.current_position as f32 * state.speed;
        view.get_interpolated(pos)
    }
    
    fn process_sample(
        sample: f32, 
        state: &HeadState, 
        processor: &SampleProcessor, 
        sample_rate: f32, 
        view: &BufferView
    ) -> f32 {
        SampleProcessor::process_sample_static(sample, state, processor, sample_rate, view)
    }
    
    fn write_sample(sample: f32, pan: f32, index: usize, outputs: &mut [&mut [f32]]) {
        SampleProcessor::write_to_outputs_static(sample, pan, index, outputs)
    }
    
    fn grain_window(position: usize, grain_size: usize) -> f32 {
        SampleProcessor::grain_window_static(position, grain_size)
    }
}

// Реализация AudioNode для интеграции с kama-core-traits
impl AudioNode for MultiHeadBuffer {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        self.process(inputs, outputs)
            .map_err(|e| AudioError::Processing(e.to_string()))
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {
        self.reset();
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 2 }  // Стерео выход
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "num_heads" => Some(ParamValue::Int(self.heads.len() as i32)),
            "buffer_size" => Some(ParamValue::Int(self.buffer_size() as i32)),
            "max_heads" => Some(ParamValue::Int(self.max_heads as i32)),
            "sample_rate" => Some(ParamValue::Float(self.sample_rate)),
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
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<MultiHeadBuffer>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "MultiHead Buffer".to_string(),
            category: NodeCategory::Effect,
            description: "Multi-head buffer for complex playback and granular synthesis".to_string(),
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
                ParamMetadata {
                    name: "buffer_size".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(self.buffer_size() as i32),
                    min: Some(64.0),
                    max: Some(65536.0),
                    step: Some(64.0),
                    unit: Some("samples".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "max_heads".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(self.max_heads as i32),
                    min: Some(1.0),
                    max: Some(32.0),
                    step: Some(1.0),
                    unit: Some("heads".to_string()),
                    choices: None,
                },
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;
    
    #[test]
    fn test_multi_head_buffer_creation() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
        
        let head1_id = buffer.add_head();
        let head2_id = buffer.add_head();
        
        assert_eq!(head1_id, 1);
        assert_eq!(head2_id, 2);
        assert_eq!(buffer.heads.len(), 2);
    }
    
    #[test]
    fn test_head_parameters() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
        
        let head_id = buffer.add_head();
        if let Some(head) = buffer.get_head_mut(head_id) {
            head.state.speed = 0.5;
            head.state.pan = -0.5;
            head.state.volume = 0.7;
        }
        
        if let Some(head) = buffer.get_head(head_id) {
            assert_eq!(head.state.speed, 0.5);
            assert_eq!(head.state.pan, -0.5);
            assert_eq!(head.state.volume, 0.7);
        }
    }
    
    #[test]
    fn test_remove_head() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
        
        buffer.add_head();
        buffer.add_head();
        buffer.add_head();
        
        assert_eq!(buffer.heads.len(), 3);
        
        buffer.remove_head(2).unwrap();
        assert_eq!(buffer.heads.len(), 2);
        
        // Проверяем перенумерацию
        if let Some(head) = buffer.get_head(2) {
            assert_eq!(head.id, 2);
        }
    }
    
    #[test]
    fn test_process_simple() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
        buffer.add_head();
        
        // Записываем тестовые данные
        let test_data: Vec<f32> = (0..256).map(|i| i as f32 / 255.0).collect();
        buffer.write(&test_data);
        
        let mut output_left = vec![0.0f32; 64];
        let mut output_right = vec![0.0f32; 64];
        let mut outputs = [&mut output_left[..], &mut output_right[..]];
        
        let result = buffer.process(&[], &mut outputs);
        assert!(result.is_ok());
        
        // Проверяем, что что-то записалось
        let has_signal = output_left.iter().any(|&x| x != 0.0) || 
                        output_right.iter().any(|&x| x != 0.0);
        assert!(has_signal);
    }
    
    #[test]
    fn test_granular_mode() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
        let head_id = buffer.add_head();
        
        if let Some(head) = buffer.get_head_mut(head_id) {
            head.read_mode = ReadMode::Granular {
                grain_size: 64,
                grain_spacing: 128,
                randomization: 0.3,
            };
        }
        
        let test_data: Vec<f32> = (0..512).map(|i| (i as f32 / 511.0) * 2.0 - 1.0).collect();
        buffer.write(&test_data);
        
        let mut output = vec![0.0f32; 128];
        let mut outputs = [&mut output[..]];
        
        let result = buffer.process(&[], &mut outputs);
        assert!(result.is_ok());
    }


    #[test]
    fn test_multi_head_buffer_write_read() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
        let head_id = buffer.add_head();
        
        // Записываем тестовый сигнал
        let test_signal: Vec<f32> = (0..256).map(|i| (i as f32 / 255.0) * 2.0 - 1.0).collect();
        buffer.write(&test_signal);
        
        // Читаем через головку
        let mut output = vec![0.0f32; 64];
        let mut outputs = [&mut output[..]];
        buffer.process(&[], &mut outputs).unwrap();
        
        // Проверяем, что что-то прочиталось
        assert!(output.iter().any(|&x| x != 0.0));
    }
    
    #[test]
    fn test_multi_head_buffer_multiple_heads() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
        
        // Добавляем несколько головок с разными параметрами
        let head1 = buffer.add_head();
        let head2 = buffer.add_head();
        let head3 = buffer.add_head();
        
        if let Some(head) = buffer.get_head_mut(head1) {
            head.state.speed = 1.0;
            head.state.pan = -1.0;
        }
        
        if let Some(head) = buffer.get_head_mut(head2) {
            head.state.speed = 2.0;
            head.state.pan = 0.0;
        }
        
        if let Some(head) = buffer.get_head_mut(head3) {
            head.state.speed = 0.5;
            head.state.pan = 1.0;
        }
        
        // Записываем тестовый сигнал
        let test_signal: Vec<f32> = (0..256).map(|i| (i as f32 / 255.0)).collect();
        buffer.write(&test_signal);
        
        // Читаем через все головки
        let mut output_left = vec![0.0f32; 128];
        let mut output_right = vec![0.0f32; 128];
        let mut outputs = [&mut output_left[..], &mut output_right[..]];
        
        buffer.process(&[], &mut outputs).unwrap();
        
        // Проверяем, что оба канала получили сигнал
        assert!(output_left.iter().any(|&x| x != 0.0));
        assert!(output_right.iter().any(|&x| x != 0.0));
        
        // Панорамирование должно создавать разницу между каналами
        assert!(output_left != output_right);
    }
    
#[test]
fn test_multi_head_buffer_granular() {
    let mut buffer = MultiHeadBuffer::new(2048, 44100.0);
    let head_id = buffer.add_head();
    
    // Генерируем тестовый сигнал (синусоида)
    let test_signal: Vec<f32> = (0..1024)
        .map(|i| (2.0 * PI * 440.0 * i as f32 / 44100.0).sin())
        .collect();
    buffer.write(&test_signal);
    
    // Настраиваем гранулярный режим
    if let Some(head) = buffer.get_head_mut(head_id) {
        head.read_mode = ReadMode::Granular {
            grain_size: 128,
            grain_spacing: 256,
            randomization: 0.2,
        };
        head.state.volume = 0.5; // Устанавливаем громкость
    }
    
    // Обрабатываем несколько блоков
    let mut output = vec![0.0f32; 256];
    let mut outputs = [&mut output[..]];
    buffer.process(&[], &mut outputs).unwrap();
    
    // Проверяем, что есть сигнал
    assert!(output.iter().any(|&x| x != 0.0));
}
    
#[test]
fn test_multi_head_buffer_pingpong() {
    let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
    let head_id = buffer.add_head();
    
    // Записываем простой сигнал
    let test_signal: Vec<f32> = (0..256).map(|i| i as f32).collect();
    buffer.write(&test_signal);
    
    // Настраиваем PingPong режим
    if let Some(head) = buffer.get_head_mut(head_id) {
        head.read_mode = ReadMode::PingPong {
            segment_size: 64,
        };
    }
    
    let mut output = vec![0.0f32; 128];
    let mut outputs = [&mut output[..]];
    buffer.process(&[], &mut outputs).unwrap();
    
    assert!(output.iter().any(|&x| x != 0.0));
}

#[test]
fn test_multi_head_buffer_reverse() {
    let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
    let head_id = buffer.add_head();
    
    // Записываем простой различимый сигнал
    let test_signal: Vec<f32> = (0..256).map(|i| i as f32).collect();
    buffer.write(&test_signal);
    
    println!("Test signal first 10: {:?}", &test_signal[..10]);
    
    // Настраиваем Reverse режим
    if let Some(head) = buffer.get_head_mut(head_id) {
        head.read_mode = ReadMode::Reverse;
        head.state.volume = 1.0;
        head.state.speed = 1.0;
        println!("Reverse mode enabled for head {}", head.id);
    }
    
    let mut output = vec![0.0f32; 64];
    let mut outputs = [&mut output[..]];
    buffer.process(&[], &mut outputs).unwrap();
    
    println!("Output first 10: {:?}", &output[..10]);
    
    // Проверяем, что есть сигнал
    assert!(output.iter().any(|&x| x != 0.0), "No output signal generated");
}


#[test]
fn test_multi_head_buffer_disable_head() {
    let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
    let head_id = buffer.add_head();
    
    let test_signal: Vec<f32> = (0..256).map(|i| i as f32).collect();
    buffer.write(&test_signal);
    
    // Сначала проверяем, что есть сигнал
    let mut output1 = vec![0.0f32; 64];
    let mut outputs1 = [&mut output1[..]];
    buffer.process(&[], &mut outputs1).unwrap();
    assert!(output1.iter().any(|&x| x != 0.0));
    
    // Отключаем головку
    if let Some(head) = buffer.get_head_mut(head_id) {
        head.enabled = false;
    }
    
    // Теперь сигнала быть не должно
    let mut output2 = vec![0.0f32; 64];
    let mut outputs2 = [&mut output2[..]];
    buffer.process(&[], &mut outputs2).unwrap();
    assert!(output2.iter().all(|&x| x == 0.0));
}
    #[test]
    fn test_multi_head_buffer_max_heads() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
        
        // Добавляем максимальное количество головок
        for i in 0..8 {
            let id = buffer.add_head();
            assert_eq!(id, i + 1);
        }
        
        // Попытка добавить ещё одну должна вернуть 0
        let id = buffer.add_head();
        assert_eq!(id, 0);
        
        // Удаляем головку
        buffer.remove_head(5).unwrap();
        
        // Теперь можно добавить новую
        let id = buffer.add_head();
        assert_ne!(id, 0);
    }
    
#[test]
fn test_multi_head_buffer_process_with_storage() {
    let mut buffer = MultiHeadBuffer::new(1024, 44100.0);
    let head_id = buffer.add_head();
    
    // Настраиваем головку
    if let Some(head) = buffer.get_head_mut(head_id) {
        head.state.pan = -0.5; // левый канал громче
        head.state.volume = 0.8;
    }
    
    let test_signal: Vec<f32> = (0..256).map(|i| i as f32 / 255.0).collect();
    buffer.write(&test_signal);
    
    let mut storage = vec![0.0f32; 256 * 2]; // 2 канала по 256 семплов
    
    buffer.process_with_storage(&[], &mut storage, 2).unwrap();
    
    // Проверяем, что данные записались
    assert!(storage.iter().any(|&x| x != 0.0));
    
    // Разделяем на каналы
    let (left, right) = storage.split_at(256);
    
    // Из-за панорамирования левый канал должен быть громче
    let left_sum: f32 = left.iter().sum();
    let right_sum: f32 = right.iter().sum();
    
    // Проверяем, что левый канал действительно громче
    assert!(left_sum > right_sum, "Left sum {} should be greater than right sum {}", left_sum, right_sum);
}

}