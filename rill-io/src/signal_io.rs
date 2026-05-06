//! # IoBackendPtr — type‑erased fat pointer to `dyn IoBackend<T>`

use std::marker::PhantomData;

pub use rill_core::io::{IoBackend, IoResult};

/// Raw pointer to a `dyn IoBackend<T>`, stored as two `usize` words.
#[derive(Copy, Clone)]
pub struct IoBackendPtr<T: ?Sized + 'static>([usize; 2], PhantomData<T>);

impl<T: ?Sized + 'static> IoBackendPtr<T> {
    /// Create a null (invalid) backend pointer.
    pub fn null() -> Self {
        Self([0; 2], PhantomData)
    }

    /// Create a backend pointer from a reference.
    pub fn from_ref(r: &dyn IoBackend<T>) -> Self {
        let ptr: *const dyn IoBackend<T> = r;
        let words: [usize; 2] = unsafe { std::mem::transmute(ptr) };
        Self(words, PhantomData)
    }

    /// Returns `true` if this is a null (invalid) pointer.
    pub fn is_null(&self) -> bool {
        self.0[0] == 0 && self.0[1] == 0
    }

    /// Try to dereference as a backend reference. Returns `None` if null.
    pub fn as_ref(&self) -> Option<&'static dyn IoBackend<T>> {
        if self.is_null() {
            None
        } else {
            let ptr: *const dyn IoBackend<T> = unsafe { std::mem::transmute(self.0) };
            Some(unsafe { &*ptr })
        }
    }
}
