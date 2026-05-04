use std::collections::HashMap;

/// Convenience alias for audio I/O operations returning `Result<T, String>`.
pub type IoResult<T> = Result<T, String>;

/// Abstract audio I/O backend.
///
/// Only `Send` — no `Sync`. The RT thread calls `read_input`/`write_output`
/// concurrently with the control thread calling `stop`, but the protocol
/// guarantees: `stop()` is called after the RT thread has been joined,
/// so `&self` is never used from two threads at once for conflicting
/// operations.
pub trait AudioIo: Send {
    /// Register the processing callback invoked by the backend on each audio cycle.
    fn set_process_callback(&self, cb: Box<dyn Fn()>);
    /// Read interleaved stereo input samples into the provided left/right buffers.
    /// Returns the number of frames actually read.
    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize;
    /// Write interleaved stereo output samples from the provided left/right buffers.
    /// Returns the number of frames actually written.
    fn write_output(&self, left: &[f32], right: &[f32]) -> usize;
    /// Start the audio stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot be started.
    fn start(&self) -> IoResult<()>;
    /// Stop the audio stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot be stopped cleanly.
    fn stop(&self) -> IoResult<()>;
}

/// Send+Sync wrapper around a fat pointer to `dyn AudioIo`.
/// Stores the pointer as `usize` (two words: data + vtable) to
/// avoid wide-pointer-to-usize cast issues.
#[derive(Copy, Clone)]
pub struct AudioIoPtr(pub [usize; 2]);

unsafe impl Send for AudioIoPtr {}
unsafe impl Sync for AudioIoPtr {}

impl AudioIoPtr {
    /// Create a null pointer (all zeros).
    pub fn null() -> Self {
        Self([0; 2])
    }

    /// Convert a `&dyn AudioIo` reference into a raw pointer.
    pub fn from_ref(r: &dyn AudioIo) -> Self {
        let ptr: *const dyn AudioIo = r;
        let words: [usize; 2] = unsafe { std::mem::transmute(ptr) };
        Self(words)
    }

    /// Check whether this pointer is null.
    pub fn is_null(&self) -> bool {
        self.0[0] == 0 && self.0[1] == 0
    }

    /// Dereference back to `&dyn AudioIo`, or `None` if null.
    ///
    /// # Safety
    ///
    /// The returned reference borrows from the original allocation and
    /// is only valid while the original backend is alive.
    pub fn as_ref(&self) -> Option<&'static dyn AudioIo> {
        if self.is_null() {
            None
        } else {
            let ptr: *const dyn AudioIo = unsafe { std::mem::transmute(self.0) };
            Some(unsafe { &*ptr })
        }
    }
}

// ============================================================================
// BackendRegistry
// ============================================================================

/// Registry of named audio backends, each stored as `Box<dyn AudioIo>`.
pub struct BackendRegistry {
    backends: HashMap<String, Box<dyn AudioIo>>,
}

impl Default for BackendRegistry {
    fn default() -> Self { Self::new() }
}

impl BackendRegistry {
    /// Create an empty registry.
    pub fn new() -> Self { Self { backends: HashMap::new() } }

    /// Register a backend and return a pointer stable until registry is modified.
    pub fn register(&mut self, name: impl Into<String>, backend: Box<dyn AudioIo>) -> AudioIoPtr {
        let name = name.into();
        self.backends.insert(name.clone(), backend);
        let ptr: *const dyn AudioIo = &**self.backends.get(&name).unwrap();
        AudioIoPtr::from_ref(unsafe { &*ptr })
    }

    /// Look up a registered backend by name and return a borrow pointer.
    pub fn get_ptr(&self, name: &str) -> Option<AudioIoPtr> {
        self.backends.get(name).map(|b| AudioIoPtr::from_ref(&**b))
    }
}
