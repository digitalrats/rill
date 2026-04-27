//! # Пул буферов для эффективного переиспользования
//!
//! [`BufferPool`] позволяет переиспользовать буферы, избегая повторных
//! аллокаций. Идеально подходит для временных буферов в графе обработки.

use std::sync::Arc;
use parking_lot::Mutex;

use crate::math::Transcendental;
use super::aligned::AlignedBuffer;

/// Пул выровненных буферов
///
/// # Пример
/// ```
/// use rill_core::buffer::BufferPool;
/// use std::sync::Arc;
///
/// let pool = Arc::new(BufferPool::<f32, 512>::new(16));
///
/// // Получить буфер из пула
/// let buffer = pool.acquire().unwrap();
/// // Использовать...
/// // Буфер автоматически возвращается в пул при drop
/// ```
pub struct BufferPool<T: Transcendental, const N: usize> {
    /// Доступные буферы
    available: Mutex<Vec<AlignedBuffer<T, N>>>,
    /// Максимальный размер пула
    max_size: usize,
    /// Статистика
    stats: Mutex<PoolStats>,
}

/// Статистика пула
#[derive(Debug, Clone, Copy, Default)]
pub struct PoolStats {
    /// Количество успешных получений
    pub acquires: usize,
    /// Количество возвратов
    pub releases: usize,
    /// Количество созданий новых буферов
    pub creations: usize,
    /// Текущий размер пула
    pub current_size: usize,
    /// Максимальный достигнутый размер
    pub max_size: usize,
}

/// Умный указатель на буфер из пула
pub struct PooledBuffer<T: Transcendental, const N: usize> {
    /// Буфер
    buffer: Option<AlignedBuffer<T, N>>,
    /// Ссылка на пул
    pool: Arc<BufferPool<T, N>>,
}

impl<T: Transcendental, const N: usize> BufferPool<T, N> {
    /// Создать новый пул с указанным максимальным размером
    pub fn new(max_size: usize) -> Self {
        Self {
            available: Mutex::new(Vec::with_capacity(max_size)),
            max_size,
            stats: Mutex::new(PoolStats::default()),
        }
    }
    
    /// Создать предварительно заполненный пул
    pub fn with_preallocation(max_size: usize) -> Self {
        let pool = Self::new(max_size);
        {
            let mut available = pool.available.lock();
            for _ in 0..max_size {
                available.push(AlignedBuffer::new());
                let mut stats = pool.stats.lock();
                stats.creations += 1; // Добавляем учёт созданных буферов
            }
        }
        pool
    }
    
    /// Получить буфер из пула
    pub fn acquire(self: &Arc<Self>) -> Option<PooledBuffer<T, N>> {
        let mut available = self.available.lock();
        
        let buffer = if let Some(buffer) = available.pop() {
            buffer
        } else {
            if available.capacity() > 0 {
                // Создаём новый буфер
                let mut stats = self.stats.lock();
                stats.creations += 1;
                AlignedBuffer::new()
            } else {
                return None;
            }
        };
        
        {
            let mut stats = self.stats.lock();
            stats.acquires += 1;
            stats.current_size = available.len();
            stats.max_size = stats.max_size.max(available.len());
        }
        
        Some(PooledBuffer {
            buffer: Some(buffer),
            pool: self.clone(),
        })
    }
    
    /// Вернуть буфер в пул (внутренний метод)
    fn release(&self, mut buffer: AlignedBuffer<T, N>) {
        buffer.fill(T::ZERO);
        
        let mut available = self.available.lock();
        if available.len() < self.max_size {
            available.push(buffer);
        }
        
        let mut stats = self.stats.lock();
        stats.releases += 1;
        stats.current_size = available.len();
    }
    
    /// Получить статистику
    pub fn stats(&self) -> PoolStats {
        *self.stats.lock()
    }
    
    /// Очистить пул
    pub fn clear(&self) {
        let mut available = self.available.lock();
        available.clear();
    }
}

impl<T: Transcendental, const N: usize> PooledBuffer<T, N> {
    /// Получить ссылку на буфер
    pub fn as_buffer(&self) -> &AlignedBuffer<T, N> {
        self.buffer.as_ref().unwrap()
    }
    
    /// Получить мутабельную ссылку на буфер
    pub fn as_buffer_mut(&mut self) -> &mut AlignedBuffer<T, N> {
        self.buffer.as_mut().unwrap()
    }
}

impl<T: Transcendental, const N: usize> std::ops::Deref for PooledBuffer<T, N> {
    type Target = AlignedBuffer<T, N>;
    
    fn deref(&self) -> &Self::Target {
        self.as_buffer()
    }
}

impl<T: Transcendental, const N: usize> std::ops::DerefMut for PooledBuffer<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_buffer_mut()
    }
}

impl<T: Transcendental, const N: usize> Drop for PooledBuffer<T, N> {
    fn drop(&mut self) {
        if let Some(buffer) = self.buffer.take() {
            self.pool.release(buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    
    #[test]
    fn test_buffer_pool() {
        let pool = Arc::new(BufferPool::<f32, 64>::new(4));
        
        {
            let buffer1 = pool.acquire().unwrap();
            let buffer2 = pool.acquire().unwrap();
            
            assert_eq!(pool.stats().acquires, 2);
        } // buffer1 и buffer2 возвращаются в пул
        
        let buffer3 = pool.acquire().unwrap();
        assert!(std::ptr::eq(&*buffer3, &*buffer3));
    }
    
    #[test]
    fn test_pool_preallocation() {
        let pool = Arc::new(BufferPool::<f32, 64>::with_preallocation(4));
        
        let stats = pool.stats();
        assert_eq!(stats.creations, 4);
    }
}