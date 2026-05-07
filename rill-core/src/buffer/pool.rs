//! # Buffer pool for efficient reuse
//!
//! [`BufferPool`] allows reusing buffers, avoiding repeated
//! allocations. Ideal for temporary buffers in the processing graph.

use std::sync::Arc;
use parking_lot::Mutex;

use crate::math::Transcendental;
use super::aligned::AlignedBuffer;

/// Pool of aligned buffers
///
/// # Example
/// ```
/// use rill_core::buffer::BufferPool;
/// use std::sync::Arc;
///
/// let pool = Arc::new(BufferPool::<f32, 512>::new(16));
///
/// // Acquire a buffer from the pool
/// let buffer = pool.acquire().unwrap();
/// // Use it...
/// // Buffer is automatically returned to the pool on drop
/// ```
pub struct BufferPool<T: Transcendental, const N: usize> {
    /// Available buffers
    available: Mutex<Vec<AlignedBuffer<T, N>>>,
    /// Maximum pool size
    max_size: usize,
    /// Statistics
    stats: Mutex<PoolStats>,
}

/// Pool statistics
#[derive(Debug, Clone, Copy, Default)]
pub struct PoolStats {
    /// Number of successful acquires
    pub acquires: usize,
    /// Number of releases
    pub releases: usize,
    /// Number of new buffer creations
    pub creations: usize,
    /// Current pool size
    pub current_size: usize,
    /// Maximum size reached
    pub max_size: usize,
}

/// Smart pointer to a buffer from the pool
pub struct PooledBuffer<T: Transcendental, const N: usize> {
    /// Buffer
    buffer: Option<AlignedBuffer<T, N>>,
    /// Reference to the pool
    pool: Arc<BufferPool<T, N>>,
}

impl<T: Transcendental, const N: usize> BufferPool<T, N> {
    /// Create a new pool with the specified maximum size
    pub fn new(max_size: usize) -> Self {
        Self {
            available: Mutex::new(Vec::with_capacity(max_size)),
            max_size,
            stats: Mutex::new(PoolStats::default()),
        }
    }
    
    /// Create a pre-filled pool
    pub fn with_preallocation(max_size: usize) -> Self {
        let pool = Self::new(max_size);
        {
            let mut available = pool.available.lock();
            for _ in 0..max_size {
                available.push(AlignedBuffer::new());
                let mut stats = pool.stats.lock();
                stats.creations += 1; // Add tracking of created buffers
            }
        }
        pool
    }
    
    /// Acquire a buffer from the pool
    pub fn acquire(self: &Arc<Self>) -> Option<PooledBuffer<T, N>> {
        let mut available = self.available.lock();
        
        let buffer = if let Some(buffer) = available.pop() {
            buffer
        } else {
            if available.capacity() > 0 {
                // Create a new buffer
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
    
    /// Release a buffer back to the pool (internal method)
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
    
    /// Get statistics
    pub fn stats(&self) -> PoolStats {
        *self.stats.lock()
    }
    
    /// Clear the pool
    pub fn clear(&self) {
        let mut available = self.available.lock();
        available.clear();
    }
}

impl<T: Transcendental, const N: usize> PooledBuffer<T, N> {
    /// Get a reference to the buffer
    pub fn as_buffer(&self) -> &AlignedBuffer<T, N> {
        self.buffer.as_ref().unwrap()
    }
    
    /// Get a mutable reference to the buffer
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
        } // buffer1 and buffer2 are returned to the pool
        
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