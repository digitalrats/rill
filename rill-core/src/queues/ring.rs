//! # Кольцевая очередь с произвольным доступом
//!
//! [`RingQueue`] — гибрид между кольцевым буфером и очередью,
//! позволяющий читать данные с произвольной задержкой.

use super::{QueueError, QueueResult, QueueStats};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::cell::AtomicCell;

/// Кольцевая очередь с произвольным доступом
///
/// Позволяет читать данные не только из головы, но и с произвольной
/// задержкой. Полезно для эффектов задержки и реверберации.
#[repr(C, align(64))]
pub struct RingQueue<T: Copy, const CAP: usize> {
    /// Данные
    data: [AtomicCell<T>; CAP],
    /// Индекс записи
    write_pos: AtomicUsize,
    /// Маска для быстрого вычисления
    mask: usize,
    /// Статистика
    stats: QueueStats,
}

impl<T: Copy, const CAP: usize> RingQueue<T, CAP> {
    /// Создать новую кольцевую очередь
    pub const fn new() -> Self {
        assert!(CAP.is_power_of_two(), "CAP must be a power of two");
        
        let data = [AtomicCell::new(unsafe { std::mem::zeroed() }); CAP];
        
        Self {
            data,
            write_pos: AtomicUsize::new(0),
            mask: CAP - 1,
            stats: QueueStats::new(),
        }
    }
    
    /// Записать элемент (всегда успешно)
    pub fn push(&self, value: T) {
        let pos = self.write_pos.load(Ordering::Relaxed);
        
        unsafe {
            *self.data[pos].get() = value;
        }
        
        self.write_pos.store((pos + 1) & self.mask, Ordering::Release);
        self.stats.record_push(self.len());
    }
    
    /// Прочитать элемент с задержкой
    ///
    /// # Arguments
    /// * `delay` - задержка в семплах (0 = последний записанный)
    pub fn read_delayed(&self, delay: usize) -> T {
        assert!(delay < CAP, "Delay must be less than CAP");
        
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let read_pos = (write_pos + CAP - delay - 1) & self.mask;
        
        unsafe { *self.data[read_pos].get() }
    }
    
    /// Прочитать элемент с плавающей задержкой (линейная интерполяция)
    pub fn read_interpolated(&self, delay_frac: f64) -> T
    where
        T: From<f64> + Into<f64>,
    {
        let delay_int = delay_frac.floor() as usize;
        let frac = delay_frac.fract();
        
        let s1: f64 = self.read_delayed(delay_int).into();
        let s2: f64 = self.read_delayed(delay_int + 1).into();
        
        T::from(s1 * (1.0 - frac) + s2 * frac)
    }
    
    /// Прочитать элемент по абсолютному индексу
    pub fn read_at(&self, index: usize) -> T {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let read_pos = (write_pos + CAP - index - 1) & self.mask;
        unsafe { *self.data[read_pos].get() }
    }
    
    /// Записать массив данных
    pub fn push_slice(&self, slice: &[T]) {
        for &value in slice {
            self.push(value);
        }
    }
    
    /// Прочитать срез данных с задержкой
    pub fn read_slice_delayed(&self, delay: usize, output: &mut [T]) {
        for (i, out) in output.iter_mut().enumerate() {
            *out = self.read_delayed(delay + i);
        }
    }
    
    /// Текущая позиция записи
    pub fn write_pos(&self) -> usize {
        self.write_pos.load(Ordering::Acquire)
    }
    
    /// Ёмкость
    pub const fn capacity(&self) -> usize {
        CAP
    }
    
    /// Количество записанных элементов (не больше CAP)
    pub fn len(&self) -> usize {
        // Для кольцевого буфера всегда есть CAP элементов
        // после первого полного оборота
        CAP
    }
    
    /// Сбросить позицию записи
    pub fn reset(&self) {
        self.write_pos.store(0, Ordering::Release);
    }
    
    /// Получить статистику
    pub fn stats(&self) -> &QueueStats {
        &self.stats
    }
}

unsafe impl<T: Copy + Send, const CAP: usize> Send for RingQueue<T, CAP> {}
unsafe impl<T: Copy + Sync, const CAP: usize> Sync for RingQueue<T, CAP> {}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ring_queue_basic() {
        let queue = RingQueue::<i32, 4>::new();
        
        queue.push(1);
        queue.push(2);
        queue.push(3);
        queue.push(4);
        
        assert_eq!(queue.read_delayed(0), 4);
        assert_eq!(queue.read_delayed(1), 3);
        assert_eq!(queue.read_delayed(2), 2);
        assert_eq!(queue.read_delayed(3), 1);
    }
    
    #[test]
    fn test_ring_queue_wraparound() {
        let queue = RingQueue::<i32, 4>::new();
        
        for i in 0..10 {
            queue.push(i);
        }
        
        // После переполнения должны быть последние 4 значения
        assert_eq!(queue.read_delayed(0), 9);
        assert_eq!(queue.read_delayed(1), 8);
        assert_eq!(queue.read_delayed(2), 7);
        assert_eq!(queue.read_delayed(3), 6);
    }
    
    #[test]
    fn test_ring_queue_interpolated() {
        let queue = RingQueue::<f64, 4>::new();
        
        queue.push(1.0);
        queue.push(2.0);
        queue.push(3.0);
        queue.push(4.0);
        
        let val = queue.read_interpolated(1.5);
        assert!((val - 3.5).abs() < 0.001);
    }
}