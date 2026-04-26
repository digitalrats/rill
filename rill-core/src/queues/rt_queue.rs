//! # Главная RT-safe очередь для двухпоточной архитектуры
//!
//! [`RtQueue`] — основная очередь для коммуникации между
//! потоком управления и аудиопотоком. Объединяет функциональность
//! SPSC и MPSC очередей с удобным API.

use super::{QueueResult, QueueStatsSnapshot};
use super::spsc::SpscQueue;

/// Тип очереди
#[derive(Debug, Clone, Copy)]
pub enum QueueType {
    /// Один производитель, один потребитель (максимальная скорость)
    SingleProducer,
    /// Много производителей, один потребитель
    MultiProducer,
}

/// Главная RT-safe очередь
///
/// # Пример
/// ```
/// use rill_core::queues::RtQueue;
///
/// // Создаём очередь для команд
/// let queue = RtQueue::<i32>::new(1024);
///
/// // Поток управления (soft RT)
/// queue.push(42).unwrap();
///
/// // Аудиопоток (hard RT)
/// if let Some(cmd) = queue.pop() {
///     println!("Got command: {}", cmd);
/// }
/// ```
pub struct RtQueue<T: Copy> {
    /// Внутренняя реализация
    inner: RtQueueInner<T>,
}

enum RtQueueInner<T: Copy> {
    Spsc(SpscQueue<T, 1024>),      // Для одного производителя
    Mpsc(super::mpsc::MpscQueue<T>), // Для многих производителей
}

impl<T: Copy + Default + Send + 'static> RtQueue<T> {
    /// Создать новую очередь с фиксированным размером
    pub fn new(capacity: usize) -> Self {
        // По умолчанию используем SPSC для максимальной производительности
        if capacity <= 1024 {
            Self {
                inner: RtQueueInner::Spsc(SpscQueue::new()),
            }
        } else {
            Self {
                inner: RtQueueInner::Mpsc(super::mpsc::MpscQueue::with_capacity(capacity)),
            }
        }
    }
    
    /// Создать очередь для одного производителя
    pub fn new_spsc() -> Self {
        Self {
            inner: RtQueueInner::Spsc(SpscQueue::new()),
        }
    }
    
    /// Создать очередь для многих производителей
    pub fn new_mpsc(capacity: usize) -> Self {
        Self {
            inner: RtQueueInner::Mpsc(super::mpsc::MpscQueue::with_capacity(capacity)),
        }
    }
    
    /// Добавить элемент (из потока управления)
    pub fn push(&self, value: T) -> QueueResult<()> {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.push(value),
            RtQueueInner::Mpsc(q) => q.push(value),
        }
    }
    
    /// Извлечь элемент (из аудиопотока)
    pub fn pop(&self) -> Option<T> {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.pop(),
            RtQueueInner::Mpsc(q) => q.pop(),
        }
    }
    
    /// Текущий размер
    pub fn len(&self) -> usize {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.len(),
            RtQueueInner::Mpsc(q) => q.size(),
        }
    }
    
    /// Вместимость
    pub fn capacity(&self) -> usize {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.capacity(),
            RtQueueInner::Mpsc(q) => q.capacity(),
        }
    }
    
    /// Проверить, пуста ли очередь
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    /// Получить статистику
    pub fn stats(&self) -> QueueStatsSnapshot {
        match &self.inner {
            RtQueueInner::Spsc(q) => q.stats(),
            RtQueueInner::Mpsc(_q) => {
                // Заглушка для MPSC
                QueueStatsSnapshot {
                    pushes: 0,
                    pops: 0,
                    overflows: 0,
                    underflows: 0,
                    max_size: 0,
                }
            }
        }
    }
}

impl<T: Copy> Clone for RtQueue<T> {
    fn clone(&self) -> Self {
        // Только для MPSC очередей, SPSC не клонируются
        match &self.inner {
            RtQueueInner::Spsc(_) => panic!("Cannot clone SPSC queue"),
            RtQueueInner::Mpsc(q) => Self {
                inner: RtQueueInner::Mpsc(super::mpsc::MpscQueue::with_capacity(q.capacity())),
            },
        }
    }
}



#[allow(unsafe_code)]
unsafe impl<T: Copy + Send> Send for RtQueue<T> {}
#[allow(unsafe_code)]
unsafe impl<T: Copy + Sync> Sync for RtQueue<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_rt_queue_spsc() {
        let queue = RtQueue::<i32>::new_spsc();
        
        queue.push(42).unwrap();
        assert_eq!(queue.pop(), Some(42));
        assert_eq!(queue.pop(), None);
    }
    
    #[test]
    fn test_rt_queue_mpsc() {
        let queue = RtQueue::<i32>::new_mpsc(16);
        
        queue.push(1).unwrap();
        queue.push(2).unwrap();
        queue.push(3).unwrap();
        
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
    }
}