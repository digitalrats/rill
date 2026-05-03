use std::collections::HashMap;

pub type IoResult<T> = Result<T, String>;

/// Abstract audio I/O backend.
pub trait AudioIo {
    fn set_process_callback(&self, cb: Box<dyn Fn()>);
    fn read_input(&self, left: &mut [f32], right: &mut [f32]) -> usize;
    fn write_output(&self, left: &[f32], right: &[f32]) -> usize;
    fn start(&self) -> IoResult<()>;
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
    pub fn null() -> Self {
        Self([0; 2])
    }

    pub fn from_ref(r: &dyn AudioIo) -> Self {
        let ptr: *const dyn AudioIo = r;
        let words: [usize; 2] = unsafe { std::mem::transmute(ptr) };
        Self(words)
    }

    pub fn is_null(&self) -> bool {
        self.0[0] == 0 && self.0[1] == 0
    }

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

pub struct BackendRegistry {
    backends: HashMap<String, Box<dyn AudioIo>>,
}

impl BackendRegistry {
    pub fn new() -> Self { Self { backends: HashMap::new() } }

    /// Register a backend and return a pointer stable until registry is modified.
    pub fn register(&mut self, name: impl Into<String>, backend: Box<dyn AudioIo>) -> AudioIoPtr {
        let name = name.into();
        self.backends.insert(name.clone(), backend);
        let ptr: *const dyn AudioIo = &**self.backends.get(&name).unwrap();
        AudioIoPtr::from_ref(unsafe { &*ptr })
    }

    pub fn get_ptr(&self, name: &str) -> Option<AudioIoPtr> {
        self.backends.get(name).map(|b| AudioIoPtr::from_ref(&**b))
    }
}
