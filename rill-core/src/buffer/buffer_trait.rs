//! # Buffer trait — абстрактный аудиобуфер
//!
//! Определяет общий интерфейс для всех типов буферов, используемых
//! в графе: per-port фиксированные буферы, heap-буферы, TapeLoop.
//!
//! Размер буфера является внутренней деталью реализации — трейт
//! не параметризован размером.

use core::ops::{Deref, DerefMut};

/// Общий интерфейс для аудиобуферов произвольного размера.
///
/// Позволяет хранить буферы разных типов и размеров в едином реестре
/// ресурсов графа (`GraphResources`).
pub trait Buffer<T> {
    /// Количество сэмплов в буфере.
    fn len(&self) -> usize;

    /// Буфер пуст?
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Доступ к данным только для чтения.
    fn as_slice(&self) -> &[T];

    /// Доступ к данным для записи.
    fn as_mut_slice(&mut self) -> &mut [T];

    /// Заполнить весь буфер значением.
    fn fill(&mut self, value: T)
    where
        T: Copy;

    /// Скопировать данные из среза. Копируется `min(src.len(), self.len())` сэмплов.
    fn copy_from(&mut self, src: &[T])
    where
        T: Copy;
}

// ============================================================================
// FixedBuffer — compile-time fixed size, stack-allocated
// ============================================================================

/// Фиксированный буфер на стеке — per-port буфер по умолчанию.
///
/// Аналог старого `Buffer<T, SIZE>`.
#[derive(Debug, Clone)]
pub struct FixedBuffer<T, const SIZE: usize> {
    data: [T; SIZE],
}

impl<T: Copy + Default, const SIZE: usize> FixedBuffer<T, SIZE> {
    pub fn new() -> Self {
        Self {
            data: [T::default(); SIZE],
        }
    }

    pub fn from_array(data: [T; SIZE]) -> Self {
        Self { data }
    }

    pub fn from_slice(slice: &[T]) -> Self
    where
        T: Copy,
    {
        let mut data = [T::default(); SIZE];
        let len = slice.len().min(SIZE);
        data[..len].copy_from_slice(&slice[..len]);
        Self { data }
    }

    pub fn as_array(&self) -> &[T; SIZE] {
        &self.data
    }

    pub fn as_mut_array(&mut self) -> &mut [T; SIZE] {
        &mut self.data
    }
}

impl<T: Copy + Default, const SIZE: usize> Default for FixedBuffer<T, SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const SIZE: usize> Deref for FixedBuffer<T, SIZE> {
    type Target = [T; SIZE];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T, const SIZE: usize> DerefMut for FixedBuffer<T, SIZE> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl<T: Copy + Default, const SIZE: usize> From<[T; SIZE]> for FixedBuffer<T, SIZE> {
    fn from(data: [T; SIZE]) -> Self {
        Self::from_array(data)
    }
}

impl<T: Default + Copy, const SIZE: usize> Buffer<T> for FixedBuffer<T, SIZE> {
    fn len(&self) -> usize {
        SIZE
    }

    fn as_slice(&self) -> &[T] {
        &self.data
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.data
    }

    fn fill(&mut self, value: T)
    where
        T: Copy,
    {
        self.data.fill(value);
    }

    fn copy_from(&mut self, src: &[T])
    where
        T: Copy,
    {
        let len = src.len().min(SIZE);
        self.data[..len].copy_from_slice(&src[..len]);
    }
}

// ============================================================================
// HeapBuffer — runtime-sized, heap-allocated
// ============================================================================

/// Буфер на куче с размером, известным только во время выполнения.
///
/// Используется для ресурсов, размер которых определяется из данных
/// (загруженные сэмплы, конфигурация).
#[derive(Debug, Clone)]
pub struct HeapBuffer<T> {
    data: Vec<T>,
}

impl<T: Default + Copy> HeapBuffer<T> {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![T::default(); size],
        }
    }

    pub fn from_vec(data: Vec<T>) -> Self {
        Self { data }
    }
}

impl<T: Default + Copy> Buffer<T> for HeapBuffer<T> {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn as_slice(&self) -> &[T] {
        &self.data
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.data
    }

    fn fill(&mut self, value: T)
    where
        T: Copy,
    {
        self.data.fill(value);
    }

    fn copy_from(&mut self, src: &[T])
    where
        T: Copy,
    {
        let len = src.len().min(self.data.len());
        self.data[..len].copy_from_slice(&src[..len]);
    }
}
