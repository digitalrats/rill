//! # Атомарная ячейка для одного значения
//!
//! [`AtomicCell`] — простейшая форма коммуникации между потоками,
//! когда нужно передавать только последнее значение.

use std::sync::atomic::{AtomicPtr, Ordering};
use std::ptr;

/// Атомарная ячейка для одного значения
///
/// Позволяет одному потоку писать, а другому — читать последнее значение.
/// Потеря промежуточных значений допускается.
pub struct AtomicCell<T> {
    /// Указатель на текущее значение
    value: AtomicPtr<T>,
}

impl<T> AtomicCell<T> {
    /// Создать новую атомарную ячейку
    pub fn new() -> Self {
        Self {
            value: AtomicPtr::new(ptr::null_mut()),
        }
    }
    
    /// Создать с начальным значением
    pub fn with_initial(value: T) -> Self {
        let boxed = Box::new(value);
        let ptr = Box::into_raw(boxed);
        Self {
            value: AtomicPtr::new(ptr),
        }
    }
    
    /// Записать новое значение (перезаписывает старое)
    ///
    /// # Safety
    /// Предыдущее значение будет удалено.
    /// Должно вызываться только из одного потока.
    pub fn store(&self, new_value: T) {
        let new_ptr = Box::into_raw(Box::new(new_value));
        let old_ptr = self.value.swap(new_ptr, Ordering::AcqRel);
        
        if !old_ptr.is_null() {
            unsafe {
                drop(Box::from_raw(old_ptr));
            }
        }
    }
    
    /// Загрузить текущее значение
    pub fn load(&self) -> Option<&T> {
        let ptr = self.value.load(Ordering::Acquire);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { &*ptr })
        }
    }
    
    /// Загрузить и клонировать значение
    pub fn load_clone(&self) -> Option<T>
    where
        T: Clone,
    {
        self.load().cloned()
    }
    
    /// Загрузить и извлечь значение (заменяет на None)
    pub fn take(&self) -> Option<T> {
        let ptr = self.value.swap(ptr::null_mut(), Ordering::AcqRel);
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { *Box::from_raw(ptr) })
        }
    }
    
    /// Проверить, пуста ли ячейка
    pub fn is_empty(&self) -> bool {
        self.value.load(Ordering::Relaxed).is_null()
    }
}

impl<T> Drop for AtomicCell<T> {
    fn drop(&mut self) {
        let ptr = self.value.load(Ordering::Relaxed);
        if !ptr.is_null() {
            unsafe {
                drop(Box::from_raw(ptr));
            }
        }
    }
}

impl<T> Default for AtomicCell<T> {
    fn default() -> Self {
        Self::new()
    }
}

unsafe impl<T: Send> Send for AtomicCell<T> {}
unsafe impl<T: Sync> Sync for AtomicCell<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    
    #[test]
    fn test_atomic_cell_basic() {
        let cell = AtomicCell::new();
        assert!(cell.is_empty());
        
        cell.store(42);
        assert!(!cell.is_empty());
        assert_eq!(cell.load_clone(), Some(42));
        
        let taken = cell.take();
        assert_eq!(taken, Some(42));
        assert!(cell.is_empty());
    }
    
    #[test]
    fn test_atomic_cell_threads() {
        let cell = std::sync::Arc::new(AtomicCell::new());
        let cell_clone = cell.clone();
        
        let writer = thread::spawn(move || {
            for i in 0..10 {
                cell_clone.store(i);
                thread::sleep(std::time::Duration::from_micros(10));
            }
        });
        
        let reader = thread::spawn(move || {
            let mut last_value = -1;
            for _ in 0..20 {
                if let Some(val) = cell.load_clone() {
                    assert!(val >= last_value);
                    last_value = val;
                }
                thread::sleep(std::time::Duration::from_micros(5));
            }
        });
        
        writer.join().unwrap();
        reader.join().unwrap();
    }
}