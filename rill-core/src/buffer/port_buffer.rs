use core::ops::{Deref, DerefMut};

/// Fixed-size audio buffer owned by a port.
///
/// Wraps `[T; SIZE]` with `Deref`/`DerefMut` for zero-cost slice access.
/// The buffer is the sole owner of its data — no external references.
#[derive(Debug, Clone)]
pub struct Buffer<T, const SIZE: usize> {
    data: [T; SIZE],
}

impl<T: Copy + Default, const SIZE: usize> Buffer<T, SIZE> {
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

    pub fn as_slice(&self) -> &[T] {
        &self.data
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.data
    }

    pub fn fill(&mut self, value: T) {
        self.data.fill(value);
    }

    pub fn copy_from(&mut self, src: &[T; SIZE])
    where
        T: Copy,
    {
        self.data.copy_from_slice(src);
    }

    pub fn copy_from_slice(&mut self, src: &[T])
    where
        T: Copy,
    {
        let len = src.len().min(SIZE);
        self.data[..len].copy_from_slice(&src[..len]);
    }
}

impl<T: Copy + Default, const SIZE: usize> Default for Buffer<T, SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const SIZE: usize> Deref for Buffer<T, SIZE> {
    type Target = [T; SIZE];
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<T, const SIZE: usize> DerefMut for Buffer<T, SIZE> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl<T: Copy + Default, const SIZE: usize> From<[T; SIZE]> for Buffer<T, SIZE> {
    fn from(data: [T; SIZE]) -> Self {
        Self::from_array(data)
    }
}
