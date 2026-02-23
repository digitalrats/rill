//! # Многоголовый буфер для гранулярного синтеза и сложного воспроизведения
//!
//! Позволяет создавать несколько независимых головок воспроизведения,
//! каждая со своими параметрами (скорость, панорама, режим чтения).
//! Идеально подходит для гранулярного синтеза и сложных текстур.

use std::sync::Arc;
use parking_lot::RwLock;

use kama_core::traits::{
    AudioNode, AudioError,
    NodeMetadata, NodeCategory, NodeTypeId,  // напрямую из traits
    ParamValue, ParamType, ParamMetadata,
    NodeId,
};

use crate::{
    RingBuffer,
    BufferHead,
    BufferViewMut,
};

/// Многоголовый буфер для гранулярного синтеза и сложного воспроизведения
/// Многоголовый буфер.
///
/// Содержит внутренний кольцевой буфер и коллекцию головок воспроизведения.
pub struct MultiHeadBuffer {
    /// Внутренний кольцевой буфер
    buffer: Arc<RwLock<RingBuffer>>,
    /// Головки воспроизведения
    heads: Vec<BufferHead>,
    /// Частота дискретизации
    sample_rate: f32,
    /// Максимальное количество головок
    max_heads: usize,
    /// ID узла в графе (если подключен к BufferManager)
    node_id: Option<NodeId>,
}

impl MultiHeadBuffer {
    /// Создать новый многоголовый буфер
    /// Создать новый многоголовый буфер.
    pub fn new(size: usize, sample_rate: f32) -> Self {
        Self {
            buffer: Arc::new(RwLock::new(RingBuffer::new(size))),
            heads: Vec::new(),
            sample_rate,
            max_heads: 8,
            node_id: None,
        }
    }

    /// Установить ID узла для интеграции с BufferManager
    /// Установить ID узла для интеграции с BufferManager.
    pub fn with_node_id(mut self, node_id: NodeId) -> Self {
        self.node_id = Some(node_id);
        self
    }

    /// Добавить новую головку
    /// Добавить новую головку.
    pub fn add_head(&mut self) -> usize {
        if self.heads.len() >= self.max_heads {
            return 0;
        }

        let id = self.heads.len() + 1;
        self.heads.push(BufferHead::new(id));
        id
    }

    /// Добавить головку с параметрами
    /// Добавить головку с параметрами.
    pub fn add_head_with_params(
        &mut self,
        speed: f32,
        pan: f32,
        volume: f32,
        mode: crate::head::ReadMode,
    ) -> usize {
        if self.heads.len() >= self.max_heads {
            return 0;
        }

        let id = self.heads.len() + 1;
        let head = BufferHead::new(id)
            .with_speed(speed)
            .with_pan(pan)
            .with_volume(volume)
            .with_read_mode(mode);

        self.heads.push(head);
        id
    }

    /// Удалить головку по ID
    /// Удалить головку по ID.
    pub fn remove_head(&mut self, id: usize) -> bool {
        if id == 0 || id > self.heads.len() {
            return false;
        }

        self.heads.remove(id - 1);

        // Перенумеровываем оставшиеся головки
        for (i, head) in self.heads.iter_mut().enumerate() {
            head.id = i + 1;
        }

        true
    }

    /// Получить ссылку на головку
    /// Получить ссылку на головку.
    pub fn get_head(&self, id: usize) -> Option<&BufferHead> {
        if id == 0 || id > self.heads.len() {
            return None;
        }
        self.heads.get(id - 1)
    }

    /// Получить мутабельную ссылку на головку
    /// Получить мутабельную ссылку на головку.
    pub fn get_head_mut(&mut self, id: usize) -> Option<&mut BufferHead> {
        if id == 0 || id > self.heads.len() {
            return None;
        }
        self.heads.get_mut(id - 1)
    }

    /// Записать данные в буфер
    /// Записать данные в буфер.
    pub fn write(&mut self, samples: &[f32]) {
        self.buffer.write().write(samples);
    }

    /// Записать данные через мутабельное View
    pub fn write_with_view<F>(&mut self, f: F)
    where
        F: FnOnce(&mut BufferViewMut<'_>),
    {
        let mut buffer_guard = self.buffer.write();
        let mut view_mut = buffer_guard.view_mut();
        f(&mut view_mut);
    }

    /// Очистить буфер
    /// Очистить буфер.
    pub fn clear(&mut self) {
        self.buffer.write().reset();
    }

    /// Получить размер буфера
    /// Получить размер буфера.
    pub fn buffer_size(&self) -> usize {
        self.buffer.read().size()
    }

    /// Получить количество головок
    /// Получить количество головок.
    pub fn head_count(&self) -> usize {
        self.heads.len()
    }

    /// Получить максимальное количество головок
    /// Получить максимальное количество головок.
    pub fn max_heads(&self) -> usize {
        self.max_heads
    }

    /// Установить максимальное количество головок
    /// Установить максимальное количество головок.
    pub fn set_max_heads(&mut self, max: usize) {
        self.max_heads = max;
        if self.heads.len() > max {
            self.heads.truncate(max);
        }
    }

    /// Сбросить все головки
    /// Сбросить все головки.
    pub fn reset_heads(&mut self) {
        for head in &mut self.heads {
            head.reset();
        }
    }

    /// Сбросить конкретную головку
    /// Сбросить конкретную головку.
    pub fn reset_head(&mut self, id: usize) -> bool {
        if let Some(head) = self.get_head_mut(id) {
            head.reset();
            true
        } else {
            false
        }
    }

    /// Включить/выключить все головки
    /// Включить/выключить все головки.
    pub fn set_all_heads_enabled(&mut self, enabled: bool) {
        for head in &mut self.heads {
            head.set_enabled(enabled);
        }
    }
}

impl AudioNode for MultiHeadBuffer {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }

        // Записываем входной сигнал, если есть
        if !inputs.is_empty() {
            self.write(inputs[0]);
        }

        // Получаем View для чтения
        let buffer_guard = self.buffer.read();
        let view = buffer_guard.view();

        // Определяем количество выходных каналов
        let is_stereo = outputs.len() >= 2;

        // Обнуляем выходные буферы
        for out in outputs.iter_mut() {
            out.fill(0.0);
        }

        // Обрабатываем каждую головку
        for head in &mut self.heads {
            if !head.enabled {
                continue;
            }

            // Читаем семпл из буфера
            let sample = head.read_sample(&view);

            // Записываем в выходы с учетом панорамы
            if is_stereo {
                let (left, right) = outputs.split_at_mut(1);
                let left = &mut left[0];
                let right = &mut right[0];

                let (left_gain, right_gain) = if head.state.pan <= 0.0 {
                    (1.0, 1.0 + head.state.pan)
                } else {
                    (1.0 - head.state.pan, 1.0)
                };

                for i in 0..left.len().min(right.len()) {
                    left[i] += sample * left_gain;
                    right[i] += sample * right_gain;
                }
            } else {
                let mono = &mut outputs[0];
                for i in 0..mono.len() {
                    mono[i] += sample;
                }
            }
        }

        Ok(())
    }

    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "num_heads" => Some(ParamValue::Int(self.heads.len() as i32)),
            "max_heads" => Some(ParamValue::Int(self.max_heads as i32)),
            "buffer_size" => Some(ParamValue::Int(self.buffer_size() as i32)),
            "sample_rate" => Some(ParamValue::Float(self.sample_rate)),
            _ => None,
        }
    }

    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("num_heads", ParamValue::Int(n)) => {
                let n = n.max(0).min(self.max_heads as i32) as usize;
                while self.heads.len() < n {
                    self.add_head();
                }
                while self.heads.len() > n {
                    self.heads.pop();
                }
                Ok(())
            }
            ("max_heads", ParamValue::Int(n)) => {
                self.set_max_heads(n.max(1).min(32) as usize);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!(
                "Unknown parameter: {}",
                name
            ))),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn reset(&mut self) {
        self.clear();
        self.reset_heads();
    }

    fn num_inputs(&self) -> usize {
        1
    }

    fn num_outputs(&self) -> usize {
        2 // По умолчанию стерео
    }

    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }

    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "MultiHead Buffer".to_string(),
            category: NodeCategory::Effect,
            description: "Multi-head buffer for granular synthesis and complex playback"
                .to_string(),
            author: "Kama Buffers".to_string(),
            version: "0.2.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "num_heads".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(1),
                    min: Some(0.0),
                    max: Some(8.0),
                    step: Some(1.0),
                    unit: Some("heads".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "max_heads".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(8),
                    min: Some(1.0),
                    max: Some(32.0),
                    step: Some(1.0),
                    unit: Some("heads".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "buffer_size".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(4096),
                    min: Some(64.0),
                    max: Some(65536.0),
                    step: Some(64.0),
                    unit: Some("samples".to_string()),
                    choices: None,
                },
            ],
        }
    }
}

impl Clone for MultiHeadBuffer {
    fn clone(&self) -> Self {
        Self {
            buffer: self.buffer.clone(),
            heads: self.heads.clone(),
            sample_rate: self.sample_rate,
            max_heads: self.max_heads,
            node_id: self.node_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_multi_head_basic() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);

        // Добавляем головки
        let head1 = buffer.add_head();
        let head2 = buffer.add_head();

        assert_eq!(head1, 1);
        assert_eq!(head2, 2);
        assert_eq!(buffer.head_count(), 2);

        // Настраиваем головки
        if let Some(head) = buffer.get_head_mut(head1) {
            head.set_speed(1.0);
            head.set_pan(-0.5);
            head.set_volume(0.7);
        }

        if let Some(head) = buffer.get_head_mut(head2) {
            head.set_speed(0.5);
            head.set_pan(0.5);
            head.set_volume(0.5);
        }

        // Записываем тестовые данные - используем ненулевой сигнал
        let test_data: Vec<f32> = (0..256).map(|i| (i as f32 / 255.0) * 2.0 - 1.0).collect();
        buffer.write(&test_data);

        // Обрабатываем
        let mut output_left = vec![0.0; 64];
        let mut output_right = vec![0.0; 64];
        let mut outputs = [&mut output_left[..], &mut output_right[..]];

        buffer.process(&[], &mut outputs).unwrap();

        // Проверяем, что есть сигнал (должен быть, так как головки читают)
        let has_signal_left = output_left.iter().any(|&x| x != 0.0);
        let has_signal_right = output_right.iter().any(|&x| x != 0.0);

        if !has_signal_left && !has_signal_right {
            println!("Left channel: {:?}", &output_left[..10]);
            println!("Right channel: {:?}", &output_right[..10]);
        }

        assert!(
            has_signal_left || has_signal_right,
            "No signal detected in either channel"
        );
    }

    #[test]
    fn test_granular_mode() {
        let mut buffer = MultiHeadBuffer::new(2048, 44100.0);

        use crate::head::ReadMode;

        let head_id = buffer.add_head_with_params(
            0.5, // скорость
            0.0, // pan
            1.0, // volume
            ReadMode::Granular {
                grain_size: 128,
                spacing: 256,
                randomization: 0.2,
            },
        );

        assert!(head_id > 0);

        // Генерируем тестовый сигнал с максимальной амплитудой
        let test_signal: Vec<f32> = (0..2048)
            .map(|i| {
                let t = i as f32 / 44100.0;
                // Используем прямоугольную волну для гарантированного сигнала
                if (440.0 * t * 2.0).sin() > 0.0 {
                    1.0
                } else {
                    -1.0
                }
            })
            .collect();

        buffer.write(&test_signal);

        let mut output_left = vec![0.0; 512];
        let mut output_right = vec![0.0; 512];
        let mut outputs = [&mut output_left[..], &mut output_right[..]];

        // Обрабатываем несколько блоков
        for _ in 0..5 {
            buffer.process(&[], &mut outputs).unwrap();
        }

        // Проверяем наличие сигнала с более низким порогом
        let has_signal = output_left
            .iter()
            .chain(output_right.iter())
            .any(|&x| x.abs() > 0.001);

        if !has_signal {
            println!("First 20 samples left: {:?}", &output_left[..20]);
            println!("First 20 samples right: {:?}", &output_right[..20]);

            if let Some(head) = buffer.get_head(head_id) {
                println!(
                    "Head state: pos={}, grain_phase={}, grain_pos={}, enabled={}",
                    head.state.position,
                    head.grain_phase(),
                    head.grain_position(),
                    head.enabled
                );
            }
        }

        assert!(has_signal, "No signal detected in granular mode");
    }

    #[test]
    fn test_remove_head() {
        let mut buffer = MultiHeadBuffer::new(1024, 44100.0);

        buffer.add_head();
        buffer.add_head();
        buffer.add_head();

        assert_eq!(buffer.head_count(), 3);

        assert!(buffer.remove_head(2));
        assert_eq!(buffer.head_count(), 2);

        // Проверяем перенумерацию
        assert!(buffer.get_head(1).is_some());
        assert!(buffer.get_head(2).is_some());
        assert!(buffer.get_head(3).is_none());
    }
}
