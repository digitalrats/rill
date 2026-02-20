use crate::AudioBuffer;
use thiserror::Error;

/// Стратегия поведения при несоответствии размера буфера
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolStrategy {
    /// Вернуть ошибку (безопасно, предсказуемо)
    Error,
    /// Автоматически изменить размер буфера (удобно, но может быть неожиданно)
    Resize,
    /// Создать новый буфер нужного размера (не добавляя в пул)
    CreateNew,
    /// Расширить пул до нового размера (очищает существующие буферы)
    ResizePool,
}

#[derive(Error, Debug)]
pub enum BufferPoolError {
    #[error("Buffer size mismatch: expected {expected}, got {got}")]
    SizeMismatch { expected: usize, got: usize },
    
    #[error("Pool is empty")]
    PoolEmpty,
    
    #[error("Pool strategy error: {0}")]
    Strategy(String),
}

pub type PoolResult<T> = Result<T, BufferPoolError>;

/// Пул буферов с настраиваемой стратегией
#[derive(Clone)]
pub struct BufferPool {
    buffers: Vec<AudioBuffer>,
    size: usize,
    strategy: PoolStrategy,
}

impl BufferPool {
    pub fn new(pool_size: usize, buffer_size: usize) -> Self {
        Self::with_strategy(pool_size, buffer_size, PoolStrategy::Error)
    }
    
    pub fn with_strategy(pool_size: usize, buffer_size: usize, strategy: PoolStrategy) -> Self {
        let mut buffers = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            buffers.push(vec![0.0; buffer_size]);
        }
        
        Self { buffers, size: buffer_size, strategy }
    }
    
    /// Получить буфер стандартного размера
    pub fn acquire(&mut self) -> PoolResult<AudioBuffer> {
        self.buffers.pop().ok_or(BufferPoolError::PoolEmpty)
    }
    
    /// Получить буфер указанного размера с учётом стратегии
    pub fn acquire_with_size(&mut self, size: usize) -> PoolResult<AudioBuffer> {
        if size == self.size {
            return self.acquire();
        }
        
        match self.strategy {
            PoolStrategy::Error => Err(BufferPoolError::SizeMismatch {
                expected: self.size,
                got: size,
            }),
            
            PoolStrategy::Resize => {
                // Пытаемся получить буфер и изменить его размер
                match self.acquire() {
                    Ok(mut buffer) => {
                        buffer.resize(size, 0.0);
                        Ok(buffer)
                    }
                    Err(e) => Err(e),
                }
            }
            
            PoolStrategy::CreateNew => {
                // Создаём новый буфер, не трогая пул
                Ok(vec![0.0; size])
            }
            
            PoolStrategy::ResizePool => {
                // Изменяем размер пула и создаём новый буфер
                self.size = size;
                self.buffers.clear();
                // Создаём один буфер нового размера
                Ok(vec![0.0; size])
            }
        }

    }
    
    /// Вернуть буфер в пул
    pub fn release(&mut self, buffer: AudioBuffer) -> PoolResult<()> {
        if buffer.len() != self.size {
            match self.strategy {
                PoolStrategy::Error => Err(BufferPoolError::SizeMismatch {
                    expected: self.size,
                    got: buffer.len(),
                }),
                
                PoolStrategy::Resize => {
                    // Изменяем размер буфера и возвращаем
                    let mut buffer = buffer;
                    buffer.resize(self.size, 0.0);
                    buffer.fill(0.0);
                    self.buffers.push(buffer);
                    Ok(())
                }
                
                PoolStrategy::CreateNew => {
                    // Просто игнорируем буфер (не возвращаем в пул)
                    Ok(())
                }
                
                PoolStrategy::ResizePool => {
                    // Изменяем размер пула под буфер
                    self.size = buffer.len();
                    self.buffers.clear();
                    let mut buffer = buffer;
                    buffer.fill(0.0);
                    self.buffers.push(buffer);
                    Ok(())
                }
            }
        } else {
            // Размер совпадает - нормальное поведение
            let mut buffer = buffer;
            buffer.fill(0.0);
            self.buffers.push(buffer);
            Ok(())
        }
    }
    
    /// Получить текущую стратегию
    pub fn strategy(&self) -> PoolStrategy {
        self.strategy
    }
    
    /// Изменить стратегию
    pub fn set_strategy(&mut self, strategy: PoolStrategy) {
        self.strategy = strategy;
    }
    
    /// Получить количество доступных буферов
    pub fn available(&self) -> usize {
        self.buffers.len()
    }
    
    /// Изменить размер пула (очищает все буферы)
    pub fn resize(&mut self, new_size: usize) {
        self.size = new_size;
        self.buffers.clear();
    }

    /// Получить текущий размер буферов в пуле
    pub fn current_size(&self) -> usize {
        self.size
    }
}

#[test]
fn test_buffer_pool_strategies() {
    // Стратегия Error
    let mut pool = BufferPool::with_strategy(2, 256, PoolStrategy::Error);
    let result = pool.acquire_with_size(512);
    assert!(result.is_err());
    
    // Стратегия Resize
    let mut pool = BufferPool::with_strategy(2, 256, PoolStrategy::Resize);
    let buf = pool.acquire_with_size(512).unwrap();
    assert_eq!(buf.len(), 512);
    assert_eq!(pool.available(), 1); // один буфер остался в пуле
    
    // Стратегия CreateNew
    let mut pool = BufferPool::with_strategy(2, 256, PoolStrategy::CreateNew);
    let buf = pool.acquire_with_size(512).unwrap();
    assert_eq!(buf.len(), 512);
    assert_eq!(pool.available(), 2); // пул не изменился
    
    // Стратегия ResizePool
    let mut pool = BufferPool::with_strategy(2, 256, PoolStrategy::ResizePool);
    let buf = pool.acquire_with_size(512).unwrap();
    assert_eq!(buf.len(), 512);
    assert_eq!(pool.available(), 0); // пул очищен
    assert_eq!(pool.current_size(), 512); // размер пула изменился
}

#[test]
fn test_buffer_pool_release_with_strategy() {
    // Стратегия Error
    let mut pool = BufferPool::with_strategy(2, 256, PoolStrategy::Error);
    let buf = vec![0.0; 512];
    let result = pool.release(buf);
    assert!(result.is_err());
    assert_eq!(pool.available(), 2); // пул не изменился
    
    // Стратегия Resize
    let mut pool = BufferPool::with_strategy(2, 256, PoolStrategy::Resize);
    let buf = vec![1.0; 512];
    pool.release(buf).unwrap();
    assert_eq!(pool.available(), 3); // буфер изменён и добавлен (2 исходных + 1 новый)
    
    // Проверяем, что буфер обнулён и правильного размера
    if let Ok(buf) = pool.acquire() {
        assert_eq!(buf.len(), 256);
        assert!(buf.iter().all(|&x| x == 0.0));
    } else {
        panic!("Failed to acquire buffer");
    }
    
    // Очищаем пул для следующего теста
    let _ = pool.acquire(); // забираем один буфер
    let _ = pool.acquire(); // забираем второй
    let _ = pool.acquire(); // забираем третий (новый)
    assert_eq!(pool.available(), 0);
    
    // Стратегия CreateNew
    let mut pool = BufferPool::with_strategy(2, 256, PoolStrategy::CreateNew);
    let buf = vec![1.0; 512];
    pool.release(buf).unwrap();
    assert_eq!(pool.available(), 2); // пул не изменился, буфер не добавлен
    
    // Стратегия ResizePool
    let mut pool = BufferPool::with_strategy(2, 256, PoolStrategy::ResizePool);
    let buf = vec![1.0; 512];
    pool.release(buf).unwrap();
    assert_eq!(pool.available(), 1); // пул изменён, буфер добавлен
    assert_eq!(pool.current_size(), 512); // размер пула изменился
    
    // Проверяем, что буфер обнулён
    if let Ok(buf) = pool.acquire() {
        assert_eq!(buf.len(), 512);
        assert!(buf.iter().all(|&x| x == 0.0));
    } else {
        panic!("Failed to acquire buffer");
    }
}