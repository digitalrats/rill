//! # Неблокирующие очереди для двухпоточной архитектуры
//!
//! Этот модуль предоставляет очереди для безопасного обмена
//! данными между потоком управления (soft RT) и аудиопотоком (hard RT).
//!
//! ## Основные компоненты
//!
//! - [`SpscQueue`] — Single-producer single-consumer очередь (максимальная скорость)
//! - [`RtQueueBase`] — базовый трейт для всех очередей
//! - [`QueueError`] — ошибки операций с очередями (thiserror)
//! - [`CommandQueue`] — команды из control thread в audio thread
//! - [`OverflowPolicy`] — политики поведения при переполнении
//! - [`UnderflowPolicy`] — политики поведения при опустошении

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

// =============================================================================
// Подмодули
// =============================================================================

pub mod command;
pub mod error;
pub mod mpsc;
pub mod observer;
pub mod ring;
pub mod rt_queue;
pub mod signal;
pub mod spsc;
pub mod telemetry;
pub mod telemetry_block;

pub use command::CommandQueue;
pub use error::{QueueError, QueueResult};
pub use mpsc::MpscQueue;
pub use rt_queue::RtQueue;
pub use spsc::SpscQueue;
pub use telemetry_block::TelemetryBlock;

// Re-export key signal types
pub use signal::{
    AutomatonCommand, CalibrationKind, CommandEnum, MappingType, SensorCommand, ServoCommand,
    SetParameter, SignalSource,
};

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
// Статистика очереди
// =============================================================================

/// Живая статистика очереди (собирается внутри очереди)
pub struct QueueStats {
    pushes: AtomicUsize,
    pops: AtomicUsize,
    overflows: AtomicUsize,
    underflows: AtomicUsize,
    max_size: AtomicUsize,
}

impl QueueStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_push(&self, current_size: usize) {
        self.pushes.fetch_add(1, Ordering::Relaxed);
        let prev = self.max_size.load(Ordering::Relaxed);
        if current_size > prev {
            let _ = self.max_size.compare_exchange(
                prev,
                current_size,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    pub fn record_pop(&self) {
        self.pops.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_overflow(&self) {
        self.overflows.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_underflow(&self) {
        self.underflows.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> QueueStatsSnapshot {
        QueueStatsSnapshot {
            pushes: self.pushes.load(Ordering::Relaxed),
            pops: self.pops.load(Ordering::Relaxed),
            overflows: self.overflows.load(Ordering::Relaxed),
            underflows: self.underflows.load(Ordering::Relaxed),
            max_size: self.max_size.load(Ordering::Relaxed),
        }
    }
}

impl Default for QueueStats {
    fn default() -> Self {
        Self {
            pushes: AtomicUsize::new(0),
            pops: AtomicUsize::new(0),
            overflows: AtomicUsize::new(0),
            underflows: AtomicUsize::new(0),
            max_size: AtomicUsize::new(0),
        }
    }
}

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
/// - RT-safe (без аллокаций, без блокировок)
pub trait RtQueueBase<T>: Send + Sync {
    /// Добавить элемент в очередь
    fn push(&self, value: T) -> QueueResult<()>;

    /// Извлечь элемент из очереди
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

    #[test]
    fn test_queue_stats_record() {
        let stats = QueueStats::new();
        stats.record_push(5);
        stats.record_push(8);
        stats.record_overflow();
        stats.record_pop();

        let snap = stats.snapshot();
        assert_eq!(snap.pushes, 2);
        assert_eq!(snap.pops, 1);
        assert_eq!(snap.overflows, 1);
        assert_eq!(snap.max_size, 8);
    }
}
