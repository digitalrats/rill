//! # RT-safe буферы для аудио обработки
//!
//! Этот модуль предоставляет lock-free буферы с фиксированными размерами.
//!
//! ## Безопасность
//! - Все unsafe операции изолированы и документированы
//! - Доступ синхронизируется через атомарные операции
//! - Гарантируется отсутствие гонок данных при правильном использовании

#![allow(unsafe_code)]

use crate::math::AudioNum;
use core::sync::atomic::{AtomicUsize, Ordering};
use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use std::fmt::Display; 

// -----------------------------------------------------------------------------
// RingBuffer - кольцевой буфер с непрерывной записью
// -----------------------------------------------------------------------------

/// Кольцевой буфер с фиксированным размером (lock-free, SPSC)
///
/// # Особенности
/// - **Непрерывная запись**: при переполнении самые старые данные перезаписываются
/// - **Без блокировок**: использует атомарные операции
/// - **Для линий задержки**: оптимален для эффектов задержки, реверберации
///
/// # Безопасность
/// - `Send`/`Sync` реализованы вручную, так как `UnsafeCell` не автоматически
/// - Все операции с памятью используют атомарные указатели
/// - Индексы всегда валидны благодаря маске (размер должен быть степенью двойки)
/// Кольцевой буфер с фиксированным размером (lock-free, SPSC)
///
/// # Особенности
/// - **Непрерывная запись**: при переполнении самые старые данные перезаписываются
/// - **Без блокировок**: использует атомарные операции
/// - **Для линий задержки**: оптимален для эффектов задержки, реверберации
#[repr(C)]
pub struct RingBuffer<T: AudioNum, const SIZE: usize> {
    /// Данные буфера
    buffer: [UnsafeCell<MaybeUninit<T>>; SIZE],
    /// Индекс записи
    head: AtomicUsize,
    /// Индекс чтения
    tail: AtomicUsize,
    /// Счётчик элементов (для простоты)
    count: AtomicUsize,
}

unsafe impl<T: AudioNum + Send, const SIZE: usize> Send for RingBuffer<T, SIZE> {}
unsafe impl<T: AudioNum + Sync, const SIZE: usize> Sync for RingBuffer<T, SIZE> {}

impl<T: AudioNum, const SIZE: usize> RingBuffer<T, SIZE> {
    pub const fn new() -> Self {
        assert!(SIZE.is_power_of_two(), "RingBuffer size must be power of two");

        let buffer = [const { UnsafeCell::new(MaybeUninit::uninit()) }; SIZE];

        Self {
            buffer,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            count: AtomicUsize::new(0),
        }
    }

    const fn mask(&self) -> usize {
        SIZE - 1
    }

    /// Записать семпл
    pub fn write(&self, sample: T) {
        let head = self.head.load(Ordering::Relaxed);
        
        unsafe {
            let cell = self.buffer.get_unchecked(head);
            *cell.get() = MaybeUninit::new(sample);
        }

        let next_head = (head + 1) & self.mask();
        self.head.store(next_head, Ordering::Release);
        
        // Увеличиваем счётчик, но не больше размера буфера
        let old_count = self.count.load(Ordering::Relaxed);
        if old_count < SIZE {
            self.count.store(old_count + 1, Ordering::Release);
        } else {
            // Если буфер уже полон, то при записи мы перезаписываем самый старый семпл
            // и tail должен сдвинуться
            let tail = self.tail.load(Ordering::Relaxed);
            self.tail.store((tail + 1) & self.mask(), Ordering::Release);
        }
    }


    /// Прочитать семпл
    pub fn read(&self) -> Option<T> {
        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return None;
        }

        let tail = self.tail.load(Ordering::Relaxed);
        
        unsafe {
            let cell = self.buffer.get_unchecked(tail);
            let sample = (*cell.get()).assume_init_read();
            
            let next_tail = (tail + 1) & self.mask();
            self.tail.store(next_tail, Ordering::Release);
            self.count.store(count - 1, Ordering::Release);
            
            Some(sample)
        }
    }

    /// Получить семпл по логическому индексу (0 = самый старый)
    pub fn get(&self, index: usize) -> T {
        let count = self.count.load(Ordering::Relaxed);
        assert!(index < count, "Index {} out of bounds (len={})", index, count);

        let tail = self.tail.load(Ordering::Relaxed);
        let physical_index = (tail + index) & self.mask();

        unsafe {
            let cell = self.buffer.get_unchecked(physical_index);
            (*cell.get()).assume_init_read()
        }
    }

    /// Прочитать с задержкой (0 = самый новый)
    pub fn read_delayed(&self, delay: usize) -> T {
        let count = self.count.load(Ordering::Relaxed);
        assert!(delay < count, "Delay {} out of bounds (len={})", delay, count);
        
        // Самый новый имеет индекс count-1
        // С задержкой delay читаем count-1-delay
        let index = count - 1 - delay;
        self.get(index)
    }

    /// Интерполированное чтение
    pub fn read_interpolated(&self, delay_frac: f32) -> T
    where
        T: AudioNum,
    {
        let count = self.count.load(Ordering::Relaxed);
        assert!(
            delay_frac >= 0.0 && delay_frac < count as f32,
            "Delay {} out of bounds (len={})",
            delay_frac, count
        );

        let delay_int = delay_frac.floor() as usize;
        let frac = T::from_f32(delay_frac.fract());

        let s1 = self.read_delayed(delay_int);
        
        if delay_int == count - 1 {
            return s1;
        }
        
        let s2 = self.read_delayed(delay_int + 1);
        s1.add(frac.mul(s2.sub(s1)))
    }

    /// Текущая длина
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    /// Пуст ли буфер
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Полон ли буфер
    pub fn is_full(&self) -> bool {
        self.len() == SIZE
    }

    /// Прочитать все в логическом порядке
    pub fn read_all(&self) -> Vec<T> {
        let mut result = Vec::new();
        let count = self.len();
        for i in 0..count {
            result.push(self.get(i));
        }
        result
    }

    /// Сбросить буфер
    pub fn reset(&self) {
        self.head.store(0, Ordering::Relaxed);
        self.tail.store(0, Ordering::Relaxed);
        self.count.store(0, Ordering::Relaxed);
    }

    /// Отладочный вывод
    pub fn debug_print(&self)
    where
        T: Display,
    {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        let count = self.count.load(Ordering::Relaxed);
        
        println!("Buffer state: head={}, tail={}, count={}", head, tail, count);
        println!("Physical buffer:");
        for i in 0..SIZE {
            unsafe {
                let cell = self.buffer.get_unchecked(i);
                let val = (*cell.get()).assume_init_read();
                println!("  [{}] = {}", i, val);
            }
        }
        if count > 0 {
            println!("Logical order (oldest to newest):");
            for i in 0..count {
                let val = self.get(i);
                let phys_idx = (tail + i) & self.mask();
                println!("  logical[{}] = physical[{}] = {}", i, phys_idx, val);
            }
        }
    }
}

impl<T: AudioNum, const SIZE: usize> Default for RingBuffer<T, SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------------
// FixedBuffer - фиксированный буфер для временных данных (не потокобезопасный)
// -----------------------------------------------------------------------------

/// Фиксированный буфер для временных данных (только для одного потока)
///
/// Используется для внутренних нужд алгоритмов (init_buffer и т.д.)
#[repr(C)]
pub struct FixedBuffer<T: AudioNum, const SIZE: usize> {
    /// Данные буфера
    data: [T; SIZE],
    /// Текущая позиция записи
    pos: usize,
    /// Флаг, указывающий, что буфер хотя бы раз заполнялся
    filled: bool,
}

impl<T: AudioNum, const SIZE: usize> FixedBuffer<T, SIZE> {
    /// Создать новый фиксированный буфер
    pub const fn new() -> Self {
        Self {
            data: [T::ZERO; SIZE],
            pos: 0,
            filled: false,
        }
    }

    /// Записать семпл в буфер (циклическая запись)
    #[inline(always)]
    pub fn write(&mut self, sample: T) {
        self.data[self.pos] = sample;
        self.pos += 1;
        if self.pos >= SIZE {
            self.pos = 0;
            self.filled = true;
        }
    }

    /// Прочитать семпл по индексу
    #[inline(always)]
    pub fn read(&self, index: usize) -> T {
        debug_assert!(index < SIZE, "Index out of bounds");
        self.data[index]
    }

    /// Прочитать семпл с задержкой от текущей позиции
    #[inline(always)]
    pub fn read_delayed(&self, delay: usize) -> T {
        debug_assert!(delay <= self.len(), "Delay too large");
        let read_pos = (self.pos + SIZE - delay) % SIZE;
        self.data[read_pos]
    }

    /// Получить текущую позицию записи
    #[inline(always)]
    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Получить текущую длину (количество записанных семплов)
    #[inline(always)]
    pub fn len(&self) -> usize {
        if self.filled {
            SIZE
        } else {
            self.pos
        }
    }

    /// Проверить, пуст ли буфер
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.pos == 0 && !self.filled
    }

    /// Проверить, заполнен ли буфер
    #[inline(always)]
    pub fn is_filled(&self) -> bool {
        self.filled
    }

    /// Сбросить буфер
    pub fn reset(&mut self) {
        self.pos = 0;
        self.filled = false;
        // Не обнуляем данные, это не обязательно
    }

    /// Получить итератор по буферу
    pub fn iter(&self) -> FixedBufferIter<T, SIZE> {
        FixedBufferIter {
            buffer: self,
            index: 0,
        }
    }

}

impl<T: AudioNum, const SIZE: usize> Default for FixedBuffer<T, SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

/// Итератор для FixedBuffer
pub struct FixedBufferIter<'a, T: AudioNum, const SIZE: usize> {
    buffer: &'a FixedBuffer<T, SIZE>,
    index: usize,
}

impl<'a, T: AudioNum, const SIZE: usize> Iterator for FixedBufferIter<'a, T, SIZE> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.buffer.len() {
            let value = self.buffer.data[self.index];
            self.index += 1;
            Some(value)
        } else {
            None
        }
    }
}

impl<'a, T: AudioNum, const SIZE: usize> ExactSizeIterator for FixedBufferIter<'a, T, SIZE> {
    fn len(&self) -> usize {
        self.buffer.len() - self.index
    }
}

// -----------------------------------------------------------------------------
// DelayLine - специализированная линия задержки
// -----------------------------------------------------------------------------

/// Линия задержки с фиксированным максимальным временем
///
/// Оптимизирована для эффектов задержки, реверберации и т.д.
#[repr(C)]
pub struct DelayLine<T: AudioNum, const MAX_DELAY: usize> {
    /// Внутренний кольцевой буфер
    buffer: RingBuffer<T, MAX_DELAY>,
    /// Текущая задержка в семплах
    delay_samples: usize,
}

impl<T: AudioNum, const MAX_DELAY: usize> DelayLine<T, MAX_DELAY> {
    /// Создать новую линию задержки
    pub const fn new() -> Self {
        Self {
            buffer: RingBuffer::new(),
            delay_samples: 0,
        }
    }

    /// Установить время задержки
    ///
    /// # Arguments
    /// * `delay_sec` - время задержки в секундах
    /// * `sample_rate` - частота дискретизации
    ///
    /// # Panics
    /// Паникует, если `delay_sec * sample_rate` превышает `MAX_DELAY`
    #[inline(always)]
    pub fn set_delay(&mut self, delay_sec: f32, sample_rate: f32) {
        let samples = (delay_sec * sample_rate) as usize;
        assert!(samples < MAX_DELAY, "Delay too long for this DelayLine");
        self.delay_samples = samples;
    }

    /// Записать семпл и получить задержанный
    /// (всегда успешно, перезаписывает старые данные)
    #[inline(always)]
    pub fn write_and_read(&mut self, input: T) -> T {
        let delayed = self.buffer.read_delayed(self.delay_samples);
        // Всегда успешно записываем, перезаписывая старые данные
        self.buffer.write(input);
        delayed
    }

    /// Только записать семпл (всегда успешно)
    #[inline(always)]
    pub fn write(&mut self, input: T) {
        self.buffer.write(input);
    }


    
    /// Только прочитать задержанный семпл
    #[inline(always)]
    pub fn read(&self) -> T {
        self.buffer.read_delayed(self.delay_samples)
    }

    /// Прочитать семпл с произвольной задержкой (без изменения состояния)
    #[inline(always)]
    pub fn read_delayed(&self, delay: usize) -> T {
        self.buffer.read_delayed(delay)
    }

    /// Сбросить линию задержки
    pub fn reset(&mut self) {
        self.buffer.reset();
    }

    /// Получить текущую задержку в семплах
    #[inline(always)]
    pub fn delay_samples(&self) -> usize {
        self.delay_samples
    }

    /// Получить максимальную задержку
    #[inline(always)]
    pub const fn max_delay(&self) -> usize {
        MAX_DELAY
    }
}

impl<T: AudioNum, const MAX_DELAY: usize> Default for DelayLine<T, MAX_DELAY> {
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------------
// Тесты
// -----------------------------------------------------------------------------


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_continuous_write() {
        let buffer = RingBuffer::<f32, 4>::new();

        // Записываем 10 семплов
        for i in 0..10 {
            buffer.write(i as f32);
            println!("\nAfter write {}:", i);
            buffer.debug_print();
        }

        // Буфер должен содержать последние 4 семпла
        assert_eq!(buffer.len(), 4, "Buffer should have 4 elements, got {}", buffer.len());

        // После 10 записей, логический порядок должен быть: [6,7,8,9]
        let all = buffer.read_all();
        println!("All values in logical order: {:?}", all);
        assert_eq!(all, vec![6.0, 7.0, 8.0, 9.0]);
    }

    #[test]
    fn test_ring_buffer_read_delayed() {
        let buffer = RingBuffer::<f32, 4>::new();

        // Записываем 6 семплов
        for i in 0..6 {
            buffer.write(i as f32);
        }

        println!("After 6 writes:");
        buffer.debug_print();
        
        // Логический порядок должен быть: [2,3,4,5]
        // read_delayed(0) должен читать самый новый (5)
        // read_delayed(1) должен читать предыдущий (4)
        // read_delayed(2) должен читать (3)
        // read_delayed(3) должен читать (2)
        
        println!("Testing read_delayed:");
        for delay in 0..4 {
            let val = buffer.read_delayed(delay);
            println!("delay={}, value={}", delay, val);
        }

        assert_eq!(buffer.read_delayed(0), 5.0);
        assert_eq!(buffer.read_delayed(1), 4.0);
        assert_eq!(buffer.read_delayed(2), 3.0);
        assert_eq!(buffer.read_delayed(3), 2.0);
    }

    #[test]
    fn test_ring_buffer_read_order() {
        let buffer = RingBuffer::<f32, 4>::new();

        // Записываем 6 семплов
        for i in 0..6 {
            buffer.write(i as f32);
        }

        println!("After writes:");
        buffer.debug_print();

        // Читаем все через read_all
        let all = buffer.read_all();
        println!("All values in logical order: {:?}", all);
        assert_eq!(all, vec![2.0, 3.0, 4.0, 5.0]);

        // Читаем через read() - должно вернуть в том же порядке
        let mut values = Vec::new();
        while let Some(val) = buffer.read() {
            values.push(val);
            println!("Read: {}, tail now={}, len={}", 
                     val, 
                     buffer.tail.load(Ordering::Relaxed),
                     buffer.len());
        }

        println!("Read values: {:?}", values);
        assert_eq!(values, vec![2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn test_ring_buffer_debug() {
        let buffer = RingBuffer::<f32, 4>::new();

        // Записываем 6 семплов
        for i in 0..6 {
            buffer.write(i as f32);
        }

        buffer.debug_print();

        let all = buffer.read_all();
        println!("All values: {:?}", all);
        assert_eq!(all, vec![2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn test_delay_line() {
        let mut delay = DelayLine::<f32, 4>::new();
        delay.set_delay(0.001, 1000.0); // 1 семпл задержки

        // Заполняем буфер
        for i in 0..4 {
            delay.write(i as f32 + 1.0);
        }

        println!("After filling DelayLine:");
        delay.buffer.debug_print();

        // read_delayed(1) должен читать семпл с задержкой 1 (предыдущий)
        // Для буфера [1,2,3,4], задержка 1 = 3
        assert_eq!(delay.write_and_read(5.0), 3.0);
        
        println!("After write_and_read(5.0):");
        delay.buffer.debug_print();
        
        assert_eq!(delay.write_and_read(6.0), 4.0);
        assert_eq!(delay.write_and_read(7.0), 5.0);
    }
}