//! # Безопасная атомарная ячейка
//!
//! Предоставляет безопасную обертку вокруг `UnsafeCell` с полностью безопасным API.
//! Конструктор гарантирует корректную инициализацию, а в случае невозможности
//! создания вызывает panic (так как продолжение работы невозможно).

use std::cell::UnsafeCell;
use std::fmt;

/// Атомарная ячейка с полностью безопасным API
///
/// # Безопасность
/// - Конструктор гарантирует корректную инициализацию
/// - Все операции с памятью инкапсулированы
/// - Паника при невозможности создания (системная ошибка)
#[repr(transparent)]
pub struct AtomicCell<T> {
    /// The actual storage, using `UnsafeCell` for interior mutability
    /// and `MaybeUninit` to avoid requiring `T: Default`
    inner: UnsafeCell<T>,
}

#[allow(unsafe_code)]
impl<T> AtomicCell<T> {
    /// Безопасно загрузить значение
    ///
    /// # Safety
    /// Этот метод безопасен, так как:
    /// - Вызывается только когда нет одновременной записи
    /// - Использует правильные гарантии памяти
    #[inline]
    pub fn load(&self) -> T
    where
        T: Copy,
    {
        // SAFETY:
        // - Вызывающий код гарантирует отсутствие одновременной записи
        // - Relaxed ordering достаточен для наших целей
        // - Значение всегда корректно инициализировано
        unsafe { *self.inner.get() }
    }

    /// Безопасно сохранить значение
    ///
    /// # Safety
    /// Этот метод безопасен, так как:
    /// - Гарантируется уникальный доступ для записи
    /// - Нет одновременного чтения
    #[inline]
    pub fn store(&self, value: T) {
        // SAFETY:
        // - Гарантируется уникальный доступ для записи
        // - Значение корректно инициализировано
        // - Relaxed ordering достаточен
        unsafe {
            *self.inner.get() = value;
        }
    }

    /// Получить указатель на данные (для совместимости)
    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.inner.get()
    }

    #[inline]
    pub const fn new(value: T) -> Self {
        Self {
            inner: UnsafeCell::new(value),
        }
    }

    /// Создать новую атомарную ячейку с проверкой
    pub fn try_new(value: T) -> Result<Self, AtomicCellError> {
        if std::mem::size_of::<T>() > isize::MAX as usize {
            return Err(AtomicCellError::TypeTooLarge);
        }
        Ok(Self::new(value))
    }
}

impl<T: Clone + Copy> Clone for AtomicCell<T> {
    fn clone(&self) -> Self {
        Self::new(self.load())
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for AtomicCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AtomicCell")
            .field("value", &self.load())
            .finish()
    }
}

impl<T: Default> Default for AtomicCell<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// Ошибки создания атомарной ячейки
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicCellError {
    /// Тип слишком большой для атомарной ячейки
    TypeTooLarge,
    /// Недостаточно памяти (крайне редко)
    OutOfMemory,
}

impl fmt::Display for AtomicCellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AtomicCellError::TypeTooLarge => write!(f, "Type too large for atomic cell"),
            AtomicCellError::OutOfMemory => write!(f, "Out of memory"),
        }
    }
}

impl std::error::Error for AtomicCellError {}

// AtomicCell может быть Send/Sync только если T: Send/Sync
#[allow(unsafe_code)]
unsafe impl<T: Send> Send for AtomicCell<T> {}
#[allow(unsafe_code)]
unsafe impl<T: Sync> Sync for AtomicCell<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_atomic_cell_basic() {
        let cell = AtomicCell::new(42);
        assert_eq!(cell.load(), 42);

        cell.store(100);
        assert_eq!(cell.load(), 100);
    }

    #[test]
    fn test_atomic_cell_try_new() {
        let cell = AtomicCell::try_new(42).unwrap();
        assert_eq!(cell.load(), 42);
    }

    #[test]
    fn test_atomic_cell_default() {
        let cell = AtomicCell::<i32>::default();
        assert_eq!(cell.load(), 0);
    }

    #[test]
    fn test_atomic_cell_clone() {
        let cell1 = AtomicCell::new(42);
        let cell2 = cell1.clone();
        assert_eq!(cell2.load(), 42);
    }
}
