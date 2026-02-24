//! # Многоголовый буфер для гранулярного синтеза и сложного воспроизведения
//!
//! Позволяет создавать несколько независимых головок воспроизведения,
//! каждая со своими параметрами (скорость, панорама, режим чтения).
//! Идеально подходит для гранулярного синтеза и сложных текстур.

use std::sync::Arc;
use parking_lot::RwLock;

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
}

impl MultiHeadBuffer {

    /// Создать новый многоголовый буфер.
    pub fn new(size: usize, sample_rate: f32) -> Self {
        Self {
            buffer: Arc::new(RwLock::new(RingBuffer::new(size))),
            heads: Vec::new(),
            sample_rate,
            max_heads: 8,
        }
    }

    /// Добавить новую головку.
    pub fn add_head(&mut self) -> usize {
        if self.heads.len() >= self.max_heads {
            return 0;
        }

        let id = self.heads.len() + 1;
        self.heads.push(BufferHead::new(id));
        id
    }

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

    /// Получить ссылку на головку.
    pub fn get_head(&self, id: usize) -> Option<&BufferHead> {
        if id == 0 || id > self.heads.len() {
            return None;
        }
        self.heads.get(id - 1)
    }

    /// Получить мутабельную ссылку на головку.
    pub fn get_head_mut(&mut self, id: usize) -> Option<&mut BufferHead> {
        if id == 0 || id > self.heads.len() {
            return None;
        }
        self.heads.get_mut(id - 1)
    }

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

    /// Очистить буфер.
    pub fn clear(&mut self) {
        self.buffer.write().reset();
    }

    /// Получить размер буфера.
    pub fn buffer_size(&self) -> usize {
        self.buffer.read().size()
    }

    /// Получить количество головок.
    pub fn head_count(&self) -> usize {
        self.heads.len()
    }

    /// Получить максимальное количество головок.
    pub fn max_heads(&self) -> usize {
        self.max_heads
    }

    /// Установить максимальное количество головок.
    pub fn set_max_heads(&mut self, max: usize) {
        self.max_heads = max;
        if self.heads.len() > max {
            self.heads.truncate(max);
        }
    }

    /// Сбросить все головки.
    pub fn reset_heads(&mut self) {
        for head in &mut self.heads {
            head.reset();
        }
    }

    /// Сбросить конкретную головку.
    pub fn reset_head(&mut self, id: usize) -> bool {
        if let Some(head) = self.get_head_mut(id) {
            head.reset();
            true
        } else {
            false
        }
    }

    /// Включить/выключить все головки.
    pub fn set_all_heads_enabled(&mut self, enabled: bool) {
        for head in &mut self.heads {
            head.set_enabled(enabled);
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
        }
    }
}

