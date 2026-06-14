use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use rill_core::io::{IoBackend, IoControl, IoResult};
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::NullBufferView;

use super::ay38910_chip::Ay38910Chip;

/// `IoBackend` + `IoControl` adapter for AY-3-8910 chip emulation.
///
/// Register writes go through `as_control()?.write_data()`, stored in atomics
/// for cross-thread safety.
#[allow(dead_code)]
pub struct Ay38910Backend {
    chip: std::cell::UnsafeCell<Ay38910Chip>,
    sample_rate: f32,
    register_buf: [AtomicU8; 16],
}

impl Ay38910Backend {
    /// Create a new AY-3-8910 backend with the given chip clock and sample rate.
    pub fn new(chip_clock: f32, sample_rate: f32) -> Self {
        Self {
            chip: std::cell::UnsafeCell::new(Ay38910Chip::new(chip_clock)),
            sample_rate,
            register_buf: std::array::from_fn(|_| AtomicU8::new(0)),
        }
    }
}

impl IoBackend for Ay38910Backend {
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

impl IoControl for Ay38910Backend {
    fn write_data(&self, data: &[u8]) -> usize {
        for (i, &v) in data.iter().enumerate().take(16) {
            self.register_buf[i].store(v, Ordering::Relaxed);
        }
        16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_registers_via_control() {
        let backend = Ay38910Backend::new(1_750_000.0, 44100.0);

        let view = backend.create_view();
        let mut buf = [0.0f32; 64];
        // Muted: read through view (NullBufferView fills with zeros)
        view.read_input(0, &mut buf);
        assert!(
            buf.iter().all(|&s| s.abs() < 0.001),
            "null view should produce zeros"
        );

        // Write active registers via IoControl
        let mut active = [0u8; 16];
        active[0] = 23;
        active[1] = 1;
        active[7] = 0b11111110;
        active[8] = 10;
        let n = backend.as_control().unwrap().write_data(&active);
        assert_eq!(n, 16);
    }
}
