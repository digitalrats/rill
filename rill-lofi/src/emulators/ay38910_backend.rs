use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use rill_core::io::{IoBackend, IoControl, IoResult};

use super::ay38910_chip::Ay38910Chip;

/// `IoBackend<f32>` + `IoControl` adapter for AY-3-8910 chip emulation.
///
/// `read()` generates audio from current register state.
/// Register writes go through `as_control()?.write_data()`, stored in atomics
/// for cross-thread safety.
pub struct Ay38910Backend {
    chip: std::cell::UnsafeCell<Ay38910Chip>,
    sample_rate: f32,
    register_buf: [AtomicU8; 16],
}

impl Ay38910Backend {
    pub fn new(chip_clock: f32, sample_rate: f32) -> Self {
        Self {
            chip: std::cell::UnsafeCell::new(Ay38910Chip::new(chip_clock)),
            sample_rate,
            register_buf: std::array::from_fn(|_| AtomicU8::new(0)),
        }
    }
}

impl IoBackend<f32> for Ay38910Backend {
    fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}

    fn read(&self, channels: &mut [&mut [f32]]) -> usize {
        let chip = unsafe { &mut *self.chip.get() };
        for i in 0..16 {
            chip.registers[i] = self.register_buf[i].load(Ordering::Relaxed);
        }
        chip.registers_dirty = true;
        let n = channels.first().map(|c| c.len()).unwrap_or(0);
        for i in 0..n {
            let s = chip.generate_sample(self.sample_rate);
            for ch in channels.iter_mut() {
                ch[i] = s;
            }
        }
        n
    }

    fn write(&self, _channels: &[&[f32]]) -> usize {
        0
    }

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

// Safety: UnsafeCell access guarded by single-threaded graph invariant
unsafe impl Send for Ay38910Backend {}
unsafe impl Sync for Ay38910Backend {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_write_read_roundtrip() {
        let backend = Ay38910Backend::new(1_750_000.0, 44100.0);
        let mut regs = [0u8; 16];
        regs[0] = 23;
        regs[1] = 1; // tone period 279
        regs[7] = 0b11111110; // Ch A tone on
        regs[8] = 10; // volume
        let ctrl = backend.as_control().unwrap();
        ctrl.write_data(&regs);

        let mut buf = [0.0f32; 64];
        backend.read(&mut [&mut buf[..]]);
        assert!(buf.iter().any(|&s| s > 0.0), "should produce audio");
    }

    #[test]
    fn test_backend_reads_latest_registers() {
        let backend = Ay38910Backend::new(1_750_000.0, 44100.0);

        // First write: all muted
        let mute = [0u8; 16];
        backend.as_control().unwrap().write_data(&mute);

        let mut buf = [0.0f32; 64];
        backend.read(&mut [&mut buf[..]]);
        assert!(
            buf.iter().all(|&s| s.abs() < 0.001),
            "muted should be silent"
        );

        // Then write: Ch A active
        let mut active = [0u8; 16];
        active[0] = 23;
        active[1] = 1;
        active[7] = 0b11111110;
        active[8] = 10;
        backend.as_control().unwrap().write_data(&active);

        backend.read(&mut [&mut buf[..]]);
        assert!(buf.iter().any(|&s| s.abs() > 0.0), "should now have audio");
    }
}
