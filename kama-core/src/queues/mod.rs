//! # Неблокирующие очереди для двухпоточной архитектуры
//!
//! Этот модуль предоставляет lock-free очереди для безопасного обмена
//! данными между потоком управления (soft RT) и аудиопотоком (hard RT).
//!
//! ## Основные компоненты
//!
//! - [`SpscQueue`] — Single-producer single-consumer очередь (максимальная скорость)
//! - [`RtQueueBase`] — базовый трейт для всех очередей
//! - [`QueueError`] — ошибки операций с очередями
//! - [`OverflowPolicy`] — политики поведения при переполнении
//! - [`UnderflowPolicy`] — политики поведения при опустошении

use std::fmt;

pub mod spsc;

pub use spsc::SpscQueue;

// =============================================================================
// Базовые типы ошибок
// =============================================================================

/// Ошибки операций с очередью
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueError {
    /// Очередь пуста
    Empty,
    /// Очередь переполнена
    Full,
    /// Неверный индекс
    InvalidIndex,
    /// Канал закрыт
    Closed,
}

impl fmt::Display for QueueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QueueError::Empty => write!(f, "Queue is empty"),
            QueueError::Full => write!(f, "Queue is full"),
            QueueError::InvalidIndex => write!(f, "Invalid index"),
            QueueError::Closed => write!(f, "Queue is closed"),
        }
    }
}

impl std::error::Error for QueueError {}

/// Результат операций с очередью
pub type QueueResult<T> = Result<T, QueueError>;

// =============================================================================
// Политики поведения
// =============================================================================

/// Политика поведения при переполнении очереди
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowPolicy {
    /// Перезаписать самый старый элемент (кольцевой буфер)
    OverwriteOldest,
    /// Отбросить новый элемент
    DropNewest,
    /// Вызвать панику (только для отладки)
    Panic,
    /// Блокировать производителя (не для RT-потоков)
    Block,
}

/// Политика поведения при пустой очереди
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderflowPolicy {
    /// Вернуть None
    ReturnNone,
    /// Вызвать панику (только для отладки)
    Panic,
}

// =============================================================================
// Статистика очереди (упрощённая версия без атомарных типов)
// =============================================================================

/// Снимок статистики очереди
#[derive(Debug, Clone, Copy, Default)]
pub struct QueueStatsSnapshot {
    /// Количество успешных push операций
    pub pushes: usize,
    /// Количество успешных pop операций
    pub pops: usize,
    /// Количество переполнений
    pub overflows: usize,
    /// Количество опустошений
    pub underflows: usize,
    /// Максимальный достигнутый размер
    pub max_size: usize,
}

impl QueueStatsSnapshot {
    /// Создать новую статистику
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Объединить две статистики
    pub fn merge(&self, other: &Self) -> Self {
        Self {
            pushes: self.pushes + other.pushes,
            pops: self.pops + other.pops,
            overflows: self.overflows + other.overflows,
            underflows: self.underflows + other.underflows,
            max_size: self.max_size.max(other.max_size),
        }
    }
}

impl fmt::Display for QueueStatsSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "pushes: {}, pops: {}, overflows: {}, underflows: {}, max_size: {}",
            self.pushes, self.pops, self.overflows, self.underflows, self.max_size
        )
    }
}

// =============================================================================
// Базовый трейт для всех очередей
// =============================================================================

/// Базовый трейт для всех очередей, безопасных для реального времени
///
/// Все реализации должны быть:
/// - Lock-free (никаких мьютексов)
/// - Wait-free для производителя
/// - RT-safe (без аллокаций, без блокировок)
pub trait RtQueueBase<T>: Send + Sync {
    /// Добавить элемент в очередь
    ///
    /// # Arguments
    /// * `value` - значение для добавления
    ///
    /// # Returns
    /// * `Ok(())` - элемент успешно добавлен
    /// * `Err(QueueError::Full)` - очередь переполнена
    fn push(&self, value: T) -> QueueResult<()>;
    
    /// Извлечь элемент из очереди
    ///
    /// # Returns
    /// * `Some(value)` - элемент успешно извлечён
    /// * `None` - очередь пуста
    fn pop(&self) -> Option<T>;
    
    /// Текущий размер очереди
    fn len(&self) -> usize;
    
    /// Вместимость очереди
    fn capacity(&self) -> usize;
    
    /// Проверить, пуста ли очередь
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    /// Проверить, полна ли очередь
    fn is_full(&self) -> bool {
        self.len() == self.capacity()
    }
    
    /// Очистить очередь
    fn clear(&self);
}

// =============================================================================
// Вспомогательные функции
// =============================================================================

/// Проверка, является ли число степенью двойки
#[inline]
pub const fn is_power_of_two(n: usize) -> bool {
    n != 0 && (n & (n - 1)) == 0
}

/// Вычислить следующую степень двойки
#[inline]
pub const fn next_power_of_two(n: usize) -> usize {
    let mut n = n - 1;
    n |= n >> 1;
    n |= n >> 2;
    n |= n >> 4;
    n |= n >> 8;
    n |= n >> 16;
    n += 1;
    n
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_queue_error_display() {
        assert_eq!(QueueError::Empty.to_string(), "Queue is empty");
        assert_eq!(QueueError::Full.to_string(), "Queue is full");
        assert_eq!(QueueError::InvalidIndex.to_string(), "Invalid index");
        assert_eq!(QueueError::Closed.to_string(), "Queue is closed");
    }
    
    #[test]
    fn test_stats_snapshot() {
        let stats1 = QueueStatsSnapshot {
            pushes: 10,
            pops: 5,
            overflows: 1,
            underflows: 0,
            max_size: 8,
        };
        
        let stats2 = QueueStatsSnapshot {
            pushes: 20,
            pops: 15,
            overflows: 0,
            underflows: 2,
            max_size: 16,
        };
        
        let merged = stats1.merge(&stats2);
        assert_eq!(merged.pushes, 30);
        assert_eq!(merged.pops, 20);
        assert_eq!(merged.overflows, 1);
        assert_eq!(merged.underflows, 2);
        assert_eq!(merged.max_size, 16);
    }
    
    #[test]
    fn test_power_of_two() {
        assert!(is_power_of_two(1));
        assert!(is_power_of_two(2));
        assert!(is_power_of_two(4));
        assert!(is_power_of_two(8));
        assert!(is_power_of_two(16));
        assert!(!is_power_of_two(3));
        assert!(!is_power_of_two(5));
        assert!(!is_power_of_two(6));
        assert!(!is_power_of_two(7));
    }
    
    #[test]
    fn test_next_power_of_two() {
        assert_eq!(next_power_of_two(1), 1);
        assert_eq!(next_power_of_two(2), 2);
        assert_eq!(next_power_of_two(3), 4);
        assert_eq!(next_power_of_two(4), 4);
        assert_eq!(next_power_of_two(5), 8);
        assert_eq!(next_power_of_two(6), 8);
        assert_eq!(next_power_of_two(7), 8);
        assert_eq!(next_power_of_two(8), 8);
        assert_eq!(next_power_of_two(9), 16);
    }
}