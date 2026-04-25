//! # Multiple-Producer Single-Consumer очередь
//!
//! Позволяет нескольким производителям отправлять данные
//! одному потребителю. Использует атомарные операции для
//! синхронизации производителей.

use super::{QueueError, QueueResult, QueueStats, OverflowPolicy};
use std::sync::atomic::{AtomicUsize, AtomicPtr, Ordering};
use std::ptr;

/// Узел связного списка для MPSC очереди
struct Node<T> {
    value: Option<T>,
    next: AtomicPtr<Node<T>>,
}

impl<T> Node<T> {
    fn new(value: T) -> *mut Node<T> {
        Box::into_raw(Box::new(Node {
            value: Some(value),
            next: AtomicPtr::new(ptr::null_mut()),
        }))
    }
    
    fn stub() -> *mut Node<T> {
        Box::into_raw(Box::new(Node {
            value: None,
            next: AtomicPtr::new(ptr::null_mut()),
        }))
    }
}

/// Multiple-Producer Single-Consumer очередь
///
/// Реализована как lock-free очередь Майкла-Скотта.
/// Производители никогда не блокируются, потребитель
/// может ждать появления данных.
pub struct MpscQueue<T> {
    /// Голова очереди (первый элемент для чтения)
    head: AtomicPtr<Node<T>>,
    /// Хвост очереди (последний элемент для записи)
    tail: AtomicPtr<Node<T>>,
    /// Счётчик для статистики
    stats: QueueStats,
    /// Максимальный размер (0 = неограничен)
    max_capacity: usize,
    /// Текущий размер (приблизительный)
    size: AtomicUsize,
}

impl<T> MpscQueue<T> {
    /// Создать новую очередь
    pub fn new() -> Self {
        let stub = Node::<T>::stub();
        Self {
            head: AtomicPtr::new(stub),
            tail: AtomicPtr::new(stub),
            stats: QueueStats::new(),
            max_capacity: 0,
            size: AtomicUsize::new(0),
        }
    }
    
    /// Создать очередь с ограниченной ёмкостью
    pub fn with_capacity(capacity: usize) -> Self {
        let mut queue = Self::new();
        queue.max_capacity = capacity;
        queue
    }
    
    /// Добавить элемент (может вызываться из нескольких потоков)
    pub fn push(&self, value: T) -> QueueResult<()> {
        // Проверка на переполнение
        if self.max_capacity > 0 {
            let size = self.size.load(Ordering::Relaxed);
            if size >= self.max_capacity {
                self.stats.record_overflow();
                return Err(QueueError::Full);
            }
        }
        
        let node = Node::new(value);
        let mut tail = self.tail.load(Ordering::Acquire);
        
        loop {
            let next = unsafe { (*tail).next.load(Ordering::Acquire) };
            
            if next.is_null() {
                // Пытаемся добавить новый узел
                match unsafe { (*tail).next.compare_exchange_weak(
                    ptr::null_mut(),
                    node,
                    Ordering::Release,
                    Ordering::Relaxed,
                ) } {
                    Ok(_) => {
                        // Обновляем tail
                        let _ = self.tail.compare_exchange(
                            tail,
                            node,
                            Ordering::Release,
                            Ordering::Relaxed,
                        );
                        self.size.fetch_add(1, Ordering::Relaxed);
                        self.stats.record_push(self.size());
                        return Ok(());
                    }
                    Err(new_next) => {
                        // Другой поток уже добавил узел, обновляем tail
                        let _ = self.tail.compare_exchange(
                            tail,
                            new_next,
                            Ordering::Release,
                            Ordering::Relaxed,
                        );
                        tail = new_next;
                    }
                }
            } else {
                // Продвигаем tail
                let _ = self.tail.compare_exchange(
                    tail,
                    next,
                    Ordering::Release,
                    Ordering::Relaxed,
                );
                tail = next;
            }
        }
    }
    
    /// Извлечь элемент (только consumer)
    pub fn pop(&self) -> Option<T> {
        let mut head = self.head.load(Ordering::Acquire);
        
        loop {
            let tail = self.tail.load(Ordering::Acquire);
            let next = unsafe { (*head).next.load(Ordering::Acquire) };
            
            if head == tail {
                if next.is_null() {
                    // Очередь пуста
                    return None;
                }
                // Tail отстаёт, помогаем продвинуть
                let _ = self.tail.compare_exchange(
                    tail,
                    next,
                    Ordering::Release,
                    Ordering::Relaxed,
                );
            } else {
                if next.is_null() {
                    continue;
                }
                
                // Забираем значение
                let value = unsafe {
                    let node = Box::from_raw(next);
                    node.value
                };
                
                // Обновляем head
                if self.head
                    .compare_exchange(head, next, Ordering::Release, Ordering::Relaxed)
                    .is_ok()
                {
                    unsafe {
                        drop(Box::from_raw(head));
                    }
                    self.size.fetch_sub(1, Ordering::Relaxed);
                    self.stats.record_pop();
                    return value;
                }
            }
        }
    }
    
    /// Текущий размер (приблизительный)
    pub fn size(&self) -> usize {
        self.size.load(Ordering::Relaxed)
    }
    
    /// Вместимость (0 = неограничена)
    pub fn capacity(&self) -> usize {
        self.max_capacity
    }
    
    /// Проверить, пуста ли очередь
    pub fn is_empty(&self) -> bool {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Acquire);
        let next = unsafe { (*head).next.load(Ordering::Acquire) };
        
        head == tail && next.is_null()
    }
}

impl<T> Drop for MpscQueue<T> {
    fn drop(&mut self) {
        while let Some(_) = self.pop() {}
        
        let head = self.head.load(Ordering::Relaxed);
        if !head.is_null() {
            unsafe {
                drop(Box::from_raw(head));
            }
        }
    }
}

unsafe impl<T: Send> Send for MpscQueue<T> {}
unsafe impl<T: Send> Sync for MpscQueue<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    
    #[test]
    fn test_mpsc_basic() {
        let queue = MpscQueue::new();
        
        queue.push(1).unwrap();
        queue.push(2).unwrap();
        queue.push(3).unwrap();
        
        assert_eq!(queue.pop(), Some(1));
        assert_eq!(queue.pop(), Some(2));
        assert_eq!(queue.pop(), Some(3));
        assert_eq!(queue.pop(), None);
    }
    
    #[test]
    fn test_mpsc_multiple_producers() {
        let queue = std::sync::Arc::new(MpscQueue::new());
        let mut handles = vec![];
        
        for i in 0..4 {
            let queue = queue.clone();
            handles.push(thread::spawn(move || {
                for j in 0..250 {
                    queue.push(i * 1000 + j).unwrap();
                }
            }));
        }
        
        for handle in handles {
            handle.join().unwrap();
        }
        
        let mut count = 0;
        while queue.pop().is_some() {
            count += 1;
        }
        
        assert_eq!(count, 1000);
    }
}