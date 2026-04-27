use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::{OverflowPolicy, QueueError, QueueResult, QueueStatsSnapshot, RtQueueBase};
use crate::buffer::AtomicCell;

// =============================================================================
// Основная структура
// =============================================================================

#[repr(C, align(64))]
pub struct SpscQueue<T: Copy, const CAP: usize> {
    buffer: [AtomicCell<T>; CAP],
    head: AtomicUsize,
    tail: AtomicUsize,
    full: AtomicBool,
    mask: usize,
    overflow_policy: OverflowPolicy,
    default_value: Option<T>,
}

impl<T: Copy + Default, const CAP: usize> SpscQueue<T, CAP> {
    /// Создать новую очередь
    pub fn new() -> Self {
        assert!(CAP.is_power_of_two(), "CAP must be a power of two");

        let buffer = std::array::from_fn(|_| AtomicCell::new(T::default()));

        Self {
            buffer,
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            full: AtomicBool::new(false),
            mask: CAP - 1,
            overflow_policy: OverflowPolicy::OverwriteOldest,
            default_value: None,
        }
    }

    /// Создать очередь с указанными политиками
    pub fn with_policies(overflow_policy: OverflowPolicy, default_value: Option<T>) -> Self {
        let mut queue = Self::new();
        queue.overflow_policy = overflow_policy;
        queue.default_value = default_value;
        queue
    }

    /// Добавить элемент
    pub fn push(&self, value: T) -> QueueResult<()> {
        let head = self.head.load(Ordering::Relaxed);
        let next_head = (head + 1) & self.mask;

        // Проверка на переполнение
        if self.full.load(Ordering::Acquire) {
            match self.overflow_policy {
                OverflowPolicy::OverwriteOldest => {
                    // Перезаписываем самый старый элемент
                    // Сдвигаем tail, чтобы освободить место
                    let _ = self.tail.fetch_add(1, Ordering::Release) & self.mask;
                    self.full.store(false, Ordering::Release);
                }

                OverflowPolicy::DropNewest => {
                    return Err(QueueError::QueueFull);
                }

                OverflowPolicy::Panic => {
                    panic!("SpscQueue overflow (capacity: {})", CAP);
                }

                OverflowPolicy::Block => {
                    return Err(QueueError::QueueFull);
                }
            }
        }

        self.buffer[head].store(value);

        // Обновляем head
        self.head.store(next_head, Ordering::Release);

        // Если после записи head догоняет tail, значит очередь полна
        if next_head == self.tail.load(Ordering::Acquire) {
            self.full.store(true, Ordering::Release);
        }

        Ok(())
    }

    /// Извлечь элемент
    pub fn pop(&self) -> Option<T> {
        if self.is_empty() {
            return self.default_value;
        }

        let tail = self.tail.load(Ordering::Relaxed);
        let value = self.buffer[tail].load();

        let next_tail = (tail + 1) & self.mask;
        self.tail.store(next_tail, Ordering::Release);

        // После извлечения очередь уже не полна
        self.full.store(false, Ordering::Release);

        Some(value)
    }

    /// Получить элемент без удаления
    pub fn peek(&self) -> Option<T> {
        if self.is_empty() {
            None
        } else {
            let tail = self.tail.load(Ordering::Acquire);
            Some(self.buffer[tail].load())
        }
    }

    /// Текущий размер
    pub fn len(&self) -> usize {
        if self.full.load(Ordering::Acquire) {
            CAP
        } else {
            let head = self.head.load(Ordering::Acquire);
            let tail = self.tail.load(Ordering::Acquire);

            if head >= tail {
                head - tail
            } else {
                CAP - tail + head
            }
        }
    }

    /// Вместимость
    pub const fn capacity(&self) -> usize {
        CAP
    }

    /// Проверить, пуста ли очередь
    pub fn is_empty(&self) -> bool {
        !self.full.load(Ordering::Acquire)
            && self.head.load(Ordering::Acquire) == self.tail.load(Ordering::Acquire)
    }

    /// Проверить, полна ли очередь
    pub fn is_full(&self) -> bool {
        self.full.load(Ordering::Acquire)
    }

    /// Очистить очередь
    pub fn clear(&self) {
        self.head.store(0, Ordering::Relaxed);
        self.tail.store(0, Ordering::Relaxed);
        self.full.store(false, Ordering::Relaxed);
    }

    /// Получить статистику
    pub fn stats(&self) -> QueueStatsSnapshot {
        QueueStatsSnapshot::default()
    }

    /// Установить значение по умолчанию
    pub fn set_default(&mut self, value: T) {
        self.default_value = Some(value);
    }

    /// Получить политику переполнения
    pub fn overflow_policy(&self) -> OverflowPolicy {
        self.overflow_policy
    }

    /// Установить политику переполнения
    pub fn set_overflow_policy(&mut self, policy: OverflowPolicy) {
        self.overflow_policy = policy;
    }
}

impl<T: Copy + Default + Send + Sync, const CAP: usize> RtQueueBase<T> for SpscQueue<T, CAP> {
    fn push(&self, value: T) -> QueueResult<()> {
        self.push(value)
    }

    fn pop(&self) -> Option<T> {
        self.pop()
    }

    fn len(&self) -> usize {
        self.len()
    }

    fn capacity(&self) -> usize {
        CAP
    }

    fn clear(&self) {
        self.clear();
    }
}

impl<T: Copy + Default + fmt::Debug, const CAP: usize> fmt::Debug for SpscQueue<T, CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SpscQueue")
            .field("head", &self.head.load(Ordering::Relaxed))
            .field("tail", &self.tail.load(Ordering::Relaxed))
            .field("capacity", &CAP)
            .field("len", &self.len())
            .field("overflow_policy", &self.overflow_policy)
            .field("default_value", &self.default_value)
            .finish()
    }
}

impl<T: Copy + Default, const CAP: usize> Default for SpscQueue<T, CAP> {
    fn default() -> Self {
        Self::new()
    }
}
#[allow(unsafe_code)]
unsafe impl<T: Copy + Send, const CAP: usize> Send for SpscQueue<T, CAP> {}
#[allow(unsafe_code)]
unsafe impl<T: Copy + Sync, const CAP: usize> Sync for SpscQueue<T, CAP> {}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spsc_basic() {
        let queue = SpscQueue::<i32, 4>::new();

        assert!(queue.is_empty());
        assert_eq!(queue.capacity(), 4);
        assert_eq!(queue.len(), 0);

        queue.push(1).unwrap();
        assert_eq!(queue.len(), 1);
        assert!(!queue.is_empty());
        assert!(!queue.is_full()); // Не полон после 1 элемента

        queue.push(2).unwrap();
        queue.push(3).unwrap();
        queue.push(4).unwrap();

        assert!(queue.is_full()); // Полон после 4 элементов
        assert_eq!(queue.len(), 4);

        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
        assert_eq!(queue.pop(), Some(4));
        assert_eq!(queue.pop(), None);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_spsc_overwrite_policy() {
        let queue = SpscQueue::<i32, 2>::new(); // политика по умолчанию OverwriteOldest

        queue.push(1).unwrap();
        queue.push(2).unwrap();
        assert!(queue.is_full());

        // Перезаписываем самый старый (1)
        queue.push(3).unwrap();
        assert_eq!(queue.len(), 2);

        // Теперь в очереди [2, 3] (2 стал самым старым)
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_spsc_drop_newest_policy() {
        let queue = SpscQueue::<i32, 2>::with_policies(OverflowPolicy::DropNewest, None);

        queue.push(1).unwrap();
        queue.push(2).unwrap();
        assert!(queue.is_full());

        // Должно вернуть ошибку, элемент не добавляется
        assert!(queue.push(3).is_err());

        // Очередь должна содержать [1, 2] в том же порядке
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_spsc_wraparound() {
        let queue = SpscQueue::<i32, 4>::new();

        // Заполняем
        queue.push(0).unwrap();
        queue.push(1).unwrap();
        queue.push(2).unwrap();
        queue.push(3).unwrap();
        assert!(queue.is_full());

        // Извлекаем два
        assert_eq!(queue.pop(), Some(0));
        assert_eq!(queue.pop(), Some(1));

        // Добавляем два новых
        queue.push(4).unwrap();
        queue.push(5).unwrap();
        assert!(queue.is_full());

        // Проверяем порядок
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
        assert_eq!(queue.pop(), Some(4));
        assert_eq!(queue.pop(), Some(5));
        assert_eq!(queue.pop(), None);
    }

    #[test]
    fn test_spsc_peek() {
        let queue = SpscQueue::<i32, 4>::new();

        assert_eq!(queue.peek(), None);

        queue.push(42).unwrap();
        assert_eq!(queue.peek(), Some(42));
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.pop(), Some(42));
        assert_eq!(queue.peek(), None);
    }

    #[test]
    fn test_spsc_clear() {
        let queue = SpscQueue::<i32, 4>::new();

        queue.push(1).unwrap();
        queue.push(2).unwrap();
        queue.push(3).unwrap();

        assert_eq!(queue.len(), 3);

        queue.clear();
        assert_eq!(queue.len(), 0);
        assert!(queue.is_empty());
    }

    #[test]
    fn test_spsc_default_value() {
        let queue = SpscQueue::<i32, 4>::with_policies(OverflowPolicy::OverwriteOldest, Some(-1));

        assert_eq!(queue.pop(), Some(-1));

        queue.push(42).unwrap();
        assert_eq!(queue.pop(), Some(42));
        assert_eq!(queue.pop(), Some(-1));
    }

    #[test]
    fn test_spsc_policy_change() {
        let mut queue = SpscQueue::<i32, 2>::new();
        assert_eq!(queue.overflow_policy(), OverflowPolicy::OverwriteOldest);

        queue.set_overflow_policy(OverflowPolicy::DropNewest);
        assert_eq!(queue.overflow_policy(), OverflowPolicy::DropNewest);
    }

    #[test]
    #[should_panic(expected = "CAP must be a power of two")]
    fn test_spsc_invalid_capacity() {
        let _ = SpscQueue::<i32, 3>::new();
    }
}
