//! # Кольцевой буфер для задержек и эффектов
//!
//! [`RingBuffer`] реализует классический кольцевой буфер (циклический буфер)
//! с фиксированным размером. Идеально подходит для эффектов задержки,
//! реверберации, хоров и т.д.
//!
//! ## Особенности
//! - Lock-free, wait-free для производителя
//! - Фиксированный размер (должен быть степенью двойки)
//! - Поддержка чтения с задержкой
//! - Интерполяция для дробных задержек
//! - Все `unsafe` операции инкапсулированы и документированы

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::storage::AtomicCell;
use crate::math::Transcendental;

// =============================================================================
// Основная структура
// =============================================================================

/// Кольцевой буфер с фиксированным размером
///
/// # Пример
/// ```
/// use rill_core::buffer::RingBuffer;
///
/// let mut buffer = RingBuffer::<f32, 4>::new();
/// buffer.write(1.0);
/// buffer.write(2.0);
/// buffer.write(3.0);
/// buffer.write(4.0);
///
/// assert_eq!(buffer.read_delayed(0), 4.0); // последний записанный
/// assert_eq!(buffer.read_delayed(1), 3.0);
/// assert_eq!(buffer.read_delayed(2), 2.0);
/// assert_eq!(buffer.read_delayed(3), 1.0);
/// ```
#[repr(C, align(64))]
pub struct RingBuffer<T: Transcendental, const N: usize> {
    /// Данные буфера (атомарные ячейки для lock-free доступа)
    data: [AtomicCell<T>; N],

    /// Индекс записи (только producer)
    head: AtomicUsize,

    /// Индекс чтения (только consumer)
    tail: AtomicUsize,

    /// Маска для быстрого вычисления (N-1)
    mask: usize,

    /// Флаг, указывающий, что буфер полон
    full: AtomicUsize,
}

impl<T: Transcendental, const N: usize> RingBuffer<T, N> {
    /// Создать новый кольцевой буфер
    ///
    /// # Panics
    /// Паникует, если N не является степенью двойки
    pub fn new() -> Self {
        assert!(N.is_power_of_two(), "RingBuffer size must be power of two");

        // Инициализируем данные нулями с помощью AtomicCell
        let data = [const { AtomicCell::new(T::ZERO) }; N];

        Self {
            data,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            mask: N - 1,
            full: AtomicUsize::new(0),
        }
    }

    /// Записать семпл (всегда успешно, перезаписывает старые данные)
    ///
    /// # Safety
    /// Эта операция безопасна, потому что:
    /// 1. `head` уникален для производителя
    /// 2. Производитель никогда не читает из своей позиции
    /// 3. Атомарные операции гарантируют видимость
    pub fn write(&mut self, sample: T) {
        let head = self.head.load(Ordering::Relaxed);
        self.data[head].store(sample);

        let next_head = (head + 1) & self.mask;
        self.head.store(next_head, Ordering::Release);

        // Если после записи head догнал tail, значит буфер полон
        if next_head == self.tail.load(Ordering::Acquire) {
            self.full.store(1, Ordering::Release);
        }
    }

    /// Записать массив семплов
    pub fn write_slice(&mut self, samples: &[T]) {
        for &sample in samples {
            self.write(sample);
        }
    }

    /// Прочитать семпл (если есть)
    ///
    /// # Returns
    /// * `Some(sample)` - семпл успешно прочитан
    /// * `None` - буфер пуст
    pub fn read(&mut self) -> Option<T> {
        let tail = self.tail.load(Ordering::Relaxed);

        if tail == self.head.load(Ordering::Acquire) && self.full.load(Ordering::Acquire) == 0 {
            return None;
        }

        // Безопасно: мы единственный потребитель для этой позиции
        let sample = self.data[tail].load();

        let next_tail = (tail + 1) & self.mask;
        self.tail.store(next_tail, Ordering::Release);
        self.full.store(0, Ordering::Release);

        Some(sample)
    }

    /// Прочитать семпл с задержкой (без изменения указателей)
    ///
    /// # Arguments
    /// * `delay` - задержка в семплах (0 = последний записанный)
    ///
    /// # Panics
    /// Паникует, если `delay >= len()`
    pub fn read_delayed(&self, delay: usize) -> T {
        assert!(delay < self.len(), "Delay must be less than buffer length");

        let head = self.head.load(Ordering::Acquire);
        // Для delay=0 читаем последний записанный (head-1)
        // Для delay=1 читаем предпоследний (head-2) и т.д.
        let read_pos = (head + self.capacity() - delay - 1) & self.mask;

        self.data[read_pos].load()
    }

    /// Прочитать с интерполяцией (для дробных задержек)
    ///
    /// # Arguments
    /// * `delay_frac` - задержка в семплах с дробной частью
    ///
    /// # Returns
    /// Интерполированное значение (линейная интерполяция)
    pub fn read_interpolated(&self, delay_frac: f32) -> T
    where
        T: From<f32> + Into<f32>,
    {
        let delay_int = delay_frac.floor() as usize;
        let frac = delay_frac.fract();

        // If fractional part is zero, no interpolation needed
        if frac == 0.0 {
            return self.read_delayed(delay_int);
        }

        let s1: f32 = self.read_delayed(delay_int).into();
        // Interpolate towards the newer sample (delay_int - 1)
        let len = self.len();
        let prev = if delay_int == 0 {
            len - 1
        } else {
            delay_int - 1
        };
        let s2: f32 = self.read_delayed(prev).into();

        T::from(s1 * (1.0 - frac) + s2 * frac)
    }

    /// Прочитать последовательность с интерполяцией
    ///
    /// # Arguments
    /// * `start_delay` - начальная задержка
    /// * `output` - буфер для записи результата
    pub fn read_sequence_interpolated(&self, start_delay: f32, output: &mut [T])
    where
        T: From<f32> + Into<f32>,
    {
        let len = self.len();
        for i in 0..output.len() {
            let delay = start_delay + i as f32;
            if delay < len as f32 {
                output[i] = self.read_interpolated(delay);
            } else {
                output[i] = T::ZERO;
            }
        }
    }

    /// Текущий размер (количество элементов в буфере)
    pub fn len(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);

        if self.full.load(Ordering::Acquire) == 1 {
            N
        } else if head >= tail {
            head - tail
        } else {
            N - tail + head
        }
    }

    /// Вместимость буфера
    pub const fn capacity(&self) -> usize {
        N
    }

    /// Проверить, пуст ли буфер
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
            && self.full.load(Ordering::Acquire) == 0
    }

    /// Проверить, полон ли буфер
    pub fn is_full(&self) -> bool {
        self.full.load(Ordering::Acquire) == 1
    }

    /// Очистить буфер (сбросить указатели)
    pub fn clear(&mut self) {
        self.head.store(0, Ordering::Relaxed);
        self.tail.store(0, Ordering::Relaxed);
        self.full.store(0, Ordering::Relaxed);

        // Опционально: обнуляем данные для безопасности
        for i in 0..N {
            self.data[i].store(T::ZERO);
        }
    }

    /// Сбросить буфер без обнуления данных (быстрее)
    pub fn reset(&mut self) {
        self.head.store(0, Ordering::Relaxed);
        self.tail.store(0, Ordering::Relaxed);
        self.full.store(0, Ordering::Relaxed);
    }
}

// =============================================================================
// Реализация Default
// =============================================================================

impl<T: Transcendental, const N: usize> Default for RingBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Реализация Debug
// =============================================================================

impl<T: Transcendental + fmt::Debug, const N: usize> fmt::Debug for RingBuffer<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Читаем текущее состояние атомарно
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        let full = self.full.load(Ordering::Relaxed);
        let len = self.len();

        // Собираем несколько первых элементов для отладки
        let mut preview = Vec::with_capacity(4);
        for i in 0..4.min(N) {
            let val = self.data[i].load();
            preview.push(val);
        }

        f.debug_struct("RingBuffer")
            .field("head", &head)
            .field("tail", &tail)
            .field("full", &full)
            .field("len", &len)
            .field("capacity", &N)
            .field("preview", &preview)
            .finish()
    }
}

// =============================================================================
// Реализация Send/Sync (безопасно, так как AtomicCell управляет синхронизацией)
// =============================================================================
#[allow(unsafe_code)]
unsafe impl<T: Transcendental + Send, const N: usize> Send for RingBuffer<T, N> {}
#[allow(unsafe_code)]
unsafe impl<T: Transcendental + Sync, const N: usize> Sync for RingBuffer<T, N> {}

// =============================================================================
// Итератор для кольцевого буфера
// =============================================================================

/// Итератор по элементам кольцевого буфера (от самого старого к самому новому)
pub struct RingBufferIter<'a, T: Transcendental, const N: usize> {
    buffer: &'a RingBuffer<T, N>,
    pos: usize,
    end: usize,
}

// rill-core/src/buffer/ring.rs - исправляем итератор
impl<'a, T: Transcendental, const N: usize> RingBufferIter<'a, T, N> {
    fn new(buffer: &'a RingBuffer<T, N>) -> Self {
        let tail = buffer.tail.load(Ordering::Acquire);
        let head = buffer.head.load(Ordering::Acquire);
        let full = buffer.full.load(Ordering::Acquire);

        // Определяем реальную длину
        let len = if full == 1 {
            N
        } else if head >= tail {
            head - tail
        } else {
            N - tail + head
        };

        // Вычисляем end как tail + len (с учётом переполнения)
        let end = tail + len;

        Self {
            buffer,
            pos: tail,
            end,
        }
    }
}

impl<'a, T: Transcendental, const N: usize> Iterator for RingBufferIter<'a, T, N> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.end {
            None
        } else {
            let idx = self.pos & self.buffer.mask;
            let value = self.buffer.data[idx].load();
            self.pos += 1;
            Some(value)
        }
    }
}

impl<'a, T: Transcendental, const N: usize> ExactSizeIterator for RingBufferIter<'a, T, N> {
    fn len(&self) -> usize {
        self.end - self.pos
    }
}

impl<T: Transcendental, const N: usize> RingBuffer<T, N> {
    /// Получить итератор по элементам буфера (от самого старого к самому новому)
    pub fn iter(&self) -> RingBufferIter<'_, T, N> {
        RingBufferIter::new(self)
    }
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_basic() {
        let mut buffer = RingBuffer::<f32, 4>::new();

        buffer.write(1.0);
        buffer.write(2.0);
        buffer.write(3.0);
        buffer.write(4.0);

        assert!(buffer.is_full());
        assert_eq!(buffer.len(), 4);

        assert_eq!(buffer.read(), Some(1.0));
        assert_eq!(buffer.read(), Some(2.0));
        assert_eq!(buffer.read(), Some(3.0));
        assert_eq!(buffer.read(), Some(4.0));
        assert_eq!(buffer.read(), None);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_ring_buffer_wraparound() {
        let mut buffer = RingBuffer::<f32, 4>::new();

        // Записываем 10 семплов
        for i in 0..10 {
            buffer.write(i as f32);
        }

        // После 10 записей должны быть последние 4 значения
        assert_eq!(buffer.read_delayed(0), 9.0);
        assert_eq!(buffer.read_delayed(1), 8.0);
        assert_eq!(buffer.read_delayed(2), 7.0);
        assert_eq!(buffer.read_delayed(3), 6.0);
    }

    #[test]
    fn test_ring_buffer_interpolated() {
        let mut buffer = RingBuffer::<f32, 4>::new();

        buffer.write(1.0);
        buffer.write(2.0);
        buffer.write(3.0);
        buffer.write(4.0);

        let val = buffer.read_interpolated(1.5);
        assert!((val - 3.5).abs() < 0.001);
    }

    #[test]
    fn test_ring_buffer_clear() {
        let mut buffer = RingBuffer::<f32, 4>::new();

        buffer.write(1.0);
        buffer.write(2.0);

        assert!(!buffer.is_empty());
        assert_eq!(buffer.len(), 2);

        buffer.clear();
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_ring_buffer_reset() {
        let mut buffer = RingBuffer::<f32, 4>::new();

        buffer.write(1.0);
        buffer.write(2.0);
        buffer.write(3.0);

        assert_eq!(buffer.len(), 3);

        buffer.reset();
        assert!(buffer.is_empty());
        assert_eq!(buffer.len(), 0);
    }

    #[test]
    fn test_ring_buffer_iterator() {
        let mut buffer = RingBuffer::<f32, 4>::new();

        buffer.write(1.0);
        buffer.write(2.0);
        buffer.write(3.0);
        buffer.write(4.0);

        let collected: Vec<f32> = buffer.iter().collect();
        assert_eq!(collected, vec![1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn test_ring_buffer_read_sequence() {
        let mut buffer = RingBuffer::<f32, 4>::new();

        buffer.write(1.0);
        buffer.write(2.0);
        buffer.write(3.0);
        buffer.write(4.0);

        let mut output = [0.0; 4];
        buffer.read_sequence_interpolated(0.0, &mut output);
        assert_eq!(output, [4.0, 3.0, 2.0, 1.0]);
    }

    #[test]
    #[should_panic(expected = "Delay must be less than buffer length")]
    fn test_ring_buffer_invalid_delay() {
        let buffer = RingBuffer::<f32, 4>::new();
        let _ = buffer.read_delayed(4);
    }

    #[test]
    fn test_ring_buffer_write_slice() {
        let mut buffer = RingBuffer::<f32, 4>::new();

        buffer.write_slice(&[1.0, 2.0, 3.0, 4.0]);

        assert_eq!(buffer.read(), Some(1.0));
        assert_eq!(buffer.read(), Some(2.0));
        assert_eq!(buffer.read(), Some(3.0));
        assert_eq!(buffer.read(), Some(4.0));
    }
}
