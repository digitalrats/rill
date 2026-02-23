//! # Кольцевой буфер с фиксированным размером
//!
//! Реализует классический кольцевой буфер (циклический буфер) с фиксированным размером.
//! Поддерживает:
//! - запись новых семплов с затиранием старых
//! - чтение с задержкой (в прошлое)
//! - чтение с опережением (lookahead)
//! - интерполяцию для дробных задержек

//! Кольцевой буфер с фиксированным размером

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::sync::Arc;

use crate::view::{BufferView, BufferViewMut};

/// Кольцевой буфер с фиксированным размером
#[derive(Clone, Debug)]
/// Кольцевой буфер с фиксированным размером.
///
/// Размер буфера всегда округляется до степени двойки для эффективного
/// вычисления индексов через битовую маску.
pub struct RingBuffer {
    /// Внутренние данные буфера
    pub(crate) buffer: Arc<RwLock<Vec<f32>>>,
    /// Размер буфера (всегда степень двойки)
    size: usize,
    /// Текущая позиция записи
    pub(crate) write_pos: usize,
    /// Маска для быстрого вычисления остатка (size - 1)
    mask: usize,
    /// Флаг, указывающий, что буфер хотя бы раз был полностью заполнен
    pub(crate) filled: bool,
}

impl RingBuffer {
    /// Создать новый кольцевой буфер
    /// Создать новый кольцевой буфер.
    ///
    /// Размер будет округлён до ближайшей степени двойки.
    pub fn new(size: usize) -> Self {
        let size = size.next_power_of_two();
        Self {
            buffer: Arc::new(RwLock::new(vec![0.0; size])),
            size,
            write_pos: 0,
            mask: size - 1,
            filled: false,
        }
    }

    /// Получить View для чтения
    /// Получить View для чтения.
    pub fn view(&self) -> BufferView<'_> {
        BufferView::new(self)
    }

    /// Получить View для записи (если нужен мутабельный доступ)
    /// Получить View для записи.
    pub fn view_mut(&mut self) -> BufferViewMut<'_> {
        BufferViewMut::new(self)
    }

    /// Записать семплы в буфер
    /// Записать семплы в буфер.
    pub fn write(&mut self, samples: &[f32]) {
        let mut view = self.view_mut();
        view.write_slice(samples);
    }

    /// Прочитать семплы с фиксированной задержкой (в прошлое)
    /// Прочитать семплы с фиксированной задержкой (в прошлое).
    pub fn read(&self, delay_samples: usize, output: &mut [f32]) {
        let view = self.view();
        for i in 0..output.len() {
            output[i] = view.read_delayed(delay_samples, i);
        }
    }

    /// Прочитать семплы с фиксированной задержкой в будущее
    ///
    /// # Arguments
    /// * `lookahead` - количество семплов вперёд для чтения
    /// * `output` - буфер для записи результата
    /// Прочитать семплы с фиксированной задержкой в будущее (lookahead).
    pub fn read_lookahead(&self, lookahead: usize, output: &mut [f32]) {
        let view = self.view();
        for i in 0..output.len() {
            output[i] = view.read_lookahead(lookahead, i);
        }
    }

    /// Прочитать с интерполяцией (упрощенный API)
    /// Прочитать с интерполяцией (для дробных задержек).
    pub fn read_interpolated(&self, delay_samples: f32, output: &mut [f32]) {
        let view = self.view();
        view.read_sequence_interpolated(delay_samples, output);
    }

    /// Получить доступ к данным для чтения (внутреннее использование)
    pub(crate) fn read_guard(&self) -> RwLockReadGuard<'_, Vec<f32>> {
        self.buffer.read()
    }

    /// Получить доступ к данным для записи (внутреннее использование)
    pub(crate) fn write_guard(&mut self) -> RwLockWriteGuard<'_, Vec<f32>> {
        self.buffer.write()
    }

    /// Получить размер буфера
    /// Получить размер буфера.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Получить текущую позицию записи
    /// Получить текущую позицию записи.
    pub fn write_pos(&self) -> usize {
        self.write_pos
    }

    /// Получить маску (size - 1)
    /// Получить маску (size - 1) для быстрых вычислений.
    pub fn mask(&self) -> usize {
        self.mask
    }

    /// Проверить, заполнен ли буфер хотя бы раз
    /// Проверить, заполнен ли буфер хотя бы раз.
    pub fn is_filled(&self) -> bool {
        self.filled
    }

    /// Получить количество записанных семплов
    /// Получить количество записанных семплов.
    pub fn len(&self) -> usize {
        if self.filled {
            self.size
        } else {
            self.write_pos
        }
    }

    /// Сбросить буфер
    /// Сбросить буфер (очистить все данные).
    pub fn reset(&mut self) {
        let mut view = self.view_mut();
        view.clear();
    }
}
