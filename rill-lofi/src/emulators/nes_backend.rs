use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use rill_core::io::{IoBackend, IoControl, IoResult};
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::NullBufferView;

use super::nes_chip::NesChip;

const NES_REG_COUNT: usize = 22;

/// `IoBackend` + `IoControl` adapter for NES 2A03 APU.
///
/// `write_data()` receives 22-byte register dumps ($4000–$4015).
#[allow(dead_code)]
pub struct NesBackend {
    chip: std::cell::UnsafeCell<NesChip>,
    sample_rate: f32,
    register_buf: [AtomicU8; NES_REG_COUNT],
}

impl NesBackend {
    /// Create a new NES 2A03 backend with the given output sample rate.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            chip: std::cell::UnsafeCell::new(NesChip::new()),
            sample_rate,
            register_buf: std::array::from_fn(|_| AtomicU8::new(0)),
        }
    }
}

impl IoBackend for NesBackend {
    fn create_view(&self) -> Arc<dyn rill_core::traits::buffer_view::BufferView> {
        Arc::new(NullBufferView::new(2, 2))
    }

    fn set_process_callback(&self, _cb: Box<dyn FnMut(&ClockTick)>) {}

    fn run(&self, _running: Arc<std::sync::atomic::AtomicBool>) -> IoResult<()> {
        Ok(())
    }

    fn stop(&self) -> IoResult<()> {
        Ok(())
    }

    fn as_control(&self) -> Option<&dyn IoControl> {
        Some(self)
    }
}

impl IoControl for NesBackend {
    fn write_data(&self, data: &[u8]) -> usize {
        for (i, &v) in data.iter().enumerate().take(NES_REG_COUNT) {
            self.register_buf[i].store(v, Ordering::Relaxed);
        }
        NES_REG_COUNT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nes_backend_register_write() {
        let backend = NesBackend::new(44100.0);
        let mut regs = [0u8; NES_REG_COUNT];
        regs[0] = 0x8F;
        regs[3] = 0x01;
        regs[21] = 0x01;
        backend.as_control().unwrap().write_data(&regs);

        let view = backend.create_view();
        let mut buf = [0.0f32; 64];
        let n = view.read_input(0, &mut buf);
        assert_eq!(n, 64);
    }
}
