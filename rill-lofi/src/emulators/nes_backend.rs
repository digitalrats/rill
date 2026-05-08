use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use rill_core::io::{IoBackend, IoControl, IoResult};

use super::nes_chip::NesChip;

const NES_REG_COUNT: usize = 22;

/// `IoBackend<u8, f32>` + `IoControl` adapter for NES 2A03 APU.
///
/// `write_data()` receives 22-byte register dumps ($4000–$4015).
/// `read()` generates audio from current register state.
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

impl IoBackend<f32> for NesBackend {
    fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}

    fn read(&self, channels: &mut [&mut [f32]]) -> usize {
        let chip = unsafe { &mut *self.chip.get() };
        let mut regs = [0u8; NES_REG_COUNT];
        for (i, r) in regs.iter_mut().enumerate() {
            *r = self.register_buf[i].load(Ordering::Relaxed);
        }
        chip.write_registers(&regs);
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

impl IoControl for NesBackend {
    fn write_data(&self, data: &[u8]) -> usize {
        for (i, &v) in data.iter().enumerate().take(NES_REG_COUNT) {
            self.register_buf[i].store(v, Ordering::Relaxed);
        }
        NES_REG_COUNT
    }
}

unsafe impl Send for NesBackend {}
unsafe impl Sync for NesBackend {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nes_backend_roundtrip() {
        let backend = NesBackend::new(44100.0);
        let mut regs = [0u8; NES_REG_COUNT];
        regs[0] = 0x8F; // pulse1: duty=50%, vol=15
        regs[3] = 0x01; // period high
        regs[21] = 0x01; // enable pulse1
        backend.as_control().unwrap().write_data(&regs);

        let mut buf = [0.0f32; 64];
        backend.read(&mut [&mut buf[..]]);
        assert!(buf.iter().any(|&s| s.abs() > 0.0), "should produce audio");
    }
}
