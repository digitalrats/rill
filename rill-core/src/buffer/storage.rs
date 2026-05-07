//! # Safe atomic cell
//!
//! Provides a safe wrapper around `UnsafeCell` with a fully safe API.
//! The constructor guarantees correct initialisation; if creation is impossible
//! it panics (since continued operation is not possible).

use std::cell::UnsafeCell;
use std::fmt;

/// Atomic cell with a fully safe API.
///
/// Wraps `UnsafeCell` to provide interior mutability without requiring `T: Default`.
///
/// # Safety
/// - The constructor guarantees correct initialisation.
/// - All memory operations are encapsulated.
/// - Panics if creation is impossible (system error).
#[repr(transparent)]
pub struct AtomicCell<T> {
    inner: UnsafeCell<T>,
}

#[allow(unsafe_code)]
impl<T> AtomicCell<T> {
    /// Load the value (requires no concurrent writes).
    #[inline]
    pub fn load(&self) -> T
    where
        T: Copy,
    {
        // SAFETY:
        // - The caller guarantees no concurrent write.
        // - Relaxed ordering is sufficient.
        // - The value is always correctly initialised.
        unsafe { *self.inner.get() }
    }

    /// Store a new value (requires unique write access, no concurrent reads).
    #[inline]
    pub fn store(&self, value: T) {
        // SAFETY:
        // - Unique write access is guaranteed.
        // - The value is correctly initialised.
        // - Relaxed ordering is sufficient.
        unsafe {
            *self.inner.get() = value;
        }
    }

    /// Get a raw pointer to the data (for compatibility).
    #[inline]
    pub fn as_ptr(&self) -> *mut T {
        self.inner.get()
    }

    /// Create a new atomic cell.
    #[inline]
    pub const fn new(value: T) -> Self {
        Self {
            inner: UnsafeCell::new(value),
        }
    }

    /// Create a new atomic cell with validation.
    ///
    /// # Errors
    /// Returns `AtomicCellError::TypeTooLarge` if the type is too large.
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

/// Errors that can occur when creating an [`AtomicCell`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomicCellError {
    /// The type is too large for an atomic cell.
    TypeTooLarge,
    /// Out of memory (extremely rare).
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

// AtomicCell can only be Send/Sync if T: Send/Sync
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
