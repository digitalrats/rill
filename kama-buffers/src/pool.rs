//! # Пул буферов для повторного использования
//! 
//! Предоставляет механизм пулинга буферов для эффективного переиспользования памяти.
//! Буферы выдаются из пула и автоматически возвращаются при освобождении.

//! Пул буферов для повторного использования

use std::fmt;
use crate::error::{BufferError, BufferResult};

/// Стратегия поведения при несоответствии размера буфера
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// Стратегия поведения при несоответствии размера буфера.
    ///
    /// - `Error` — возвращать ошибку
    /// - `Resize` — изменить размер буфера
    /// - `CreateNew` — создать новый буфер нужного размера
    /// - `ResizePool` — изменить размер всех буферов в пуле
pub enum PoolStrategy {
    Error,
    Resize,
    CreateNew,
    ResizePool,
}

/// Пул буферов для повторного использования
    /// Пул буферов для повторного использования.
    ///
    /// Хранит вектор предварительно выделенных буферов и выдаёт их по запросу.
pub struct BufferPool {
    buffers: Vec<Vec<f32>>,
    size: usize,
    strategy: PoolStrategy,
    max_size: usize,
}

impl BufferPool {
    /// Создать новый пул
    /// Создать новый пул с указанными параметрами.
    pub fn new(max_size: usize, buffer_size: usize, strategy: PoolStrategy) -> Self {
        let mut buffers = Vec::with_capacity(max_size);
        for _ in 0..max_size {
            buffers.push(vec![0.0; buffer_size]);
        }
        
        Self {
            buffers,
            size: buffer_size,
            strategy,
            max_size,
        }
    }
    
    /// Получить буфер стандартного размера
    /// Получить буфер стандартного размера из пула.
    pub fn acquire(&mut self) -> BufferResult<Vec<f32>> {
        self.buffers.pop().ok_or(BufferError::PoolEmpty)
    }
    
    /// Получить буфер указанного размера с учётом стратегии
    /// Получить буфер указанного размера с учётом стратегии.
    pub fn acquire_with_size(&mut self, size: usize) -> BufferResult<Vec<f32>> {
        if size == self.size {
            return self.acquire();
        }
        
        match self.strategy {
            PoolStrategy::Error => Err(BufferError::SizeMismatch {
                expected: self.size,
                got: size,
            }),
            
            PoolStrategy::Resize => {
                match self.acquire() {
                    Ok(mut buffer) => {
                        buffer.resize(size, 0.0);
                        Ok(buffer)
                    }
                    Err(e) => Err(e),
                }
            }
            
            PoolStrategy::CreateNew => {
                Ok(vec![0.0; size])
            }
            
            PoolStrategy::ResizePool => {
                self.size = size;
                self.buffers.clear();
                Ok(vec![0.0; size])
            }
        }
    }
    
    /// Вернуть буфер в пул
    /// Вернуть буфер в пул.
    pub fn release(&mut self, mut buffer: Vec<f32>) -> BufferResult<()> {
        if self.buffers.len() >= self.max_size {
            return Ok(());
        }
        
        if buffer.len() != self.size {
            match self.strategy {
                PoolStrategy::Error => Err(BufferError::SizeMismatch {
                    expected: self.size,
                    got: buffer.len(),
                }),
                
                PoolStrategy::Resize => {
                    buffer.resize(self.size, 0.0);
                    buffer.fill(0.0);
                    self.buffers.push(buffer);
                    Ok(())
                }
                
                PoolStrategy::CreateNew => Ok(()),
                
                PoolStrategy::ResizePool => {
                    self.size = buffer.len();
                    self.buffers.clear();
                    buffer.fill(0.0);
                    self.buffers.push(buffer);
                    Ok(())
                }
            }
        } else {
            buffer.fill(0.0);
            self.buffers.push(buffer);
            Ok(())
        }
    }
    
    /// Получить количество доступных буферов
    /// Получить количество доступных буферов.
    pub fn available(&self) -> usize {
        self.buffers.len()
    }
    
    /// Получить текущий размер буферов в пуле
    /// Получить текущий размер буферов в пуле.
    pub fn current_size(&self) -> usize {
        self.size
    }
    
    /// Очистить пул
    /// Очистить пул.
    pub fn clear(&mut self) {
        self.buffers.clear();
    }
}

impl fmt::Debug for BufferPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BufferPool")
            .field("size", &self.size)
            .field("available", &self.available())
            .field("strategy", &self.strategy)
            .field("max_size", &self.max_size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_pool_basic() {
        let mut pool = BufferPool::new(2, 256, PoolStrategy::Resize);
        
        assert_eq!(pool.available(), 2);
        
        let buf = pool.acquire().unwrap();
        assert_eq!(buf.len(), 256);
        assert_eq!(pool.available(), 1);
        
        pool.release(buf).unwrap();
        assert_eq!(pool.available(), 2);
    }
    
    #[test]
    fn test_pool_resize() {
        let mut pool = BufferPool::new(2, 256, PoolStrategy::Resize);
        
        assert_eq!(pool.available(), 2);
        
        let buf = pool.acquire_with_size(512).unwrap();
        assert_eq!(buf.len(), 512);
        assert_eq!(pool.available(), 1);
        
        pool.release(buf).unwrap();
        assert_eq!(pool.available(), 2);
        assert_eq!(pool.current_size(), 256);
    }
}