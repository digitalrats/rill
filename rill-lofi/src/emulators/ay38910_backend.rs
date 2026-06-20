use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use rill_core::io::{IoBackend, IoControl, IoResult};
use rill_core::time::ClockTick;
use rill_core::traits::buffer_view::BufferView;

use super::ay38910_chip::Ay38910Chip;

/// BufferView that reads audio samples directly from the AY-3-8910 chip emulation.
///
/// Each `read_input` call generates `n` samples by calling
/// `chip.generate_sample(sample_rate)` and writing the register values
/// from `register_buf` into the chip before generation.
struct Ay38910View {
    chip: *const Ay38910Chip,
    register_buf: *const [AtomicU8; 16],
    sample_rate: f32,
    num_channels: usize,
}

unsafe impl Send for Ay38910View {}
unsafe impl Sync for Ay38910View {}

impl Ay38910View {
    unsafe fn generate(&self, dst: &mut [f32]) {
        let chip = &mut *(self.chip as *mut Ay38910Chip);
        let regs = &*self.register_buf;
        for (i, r) in regs.iter().enumerate() {
            chip.write_register(i, r.load(Ordering::Relaxed));
        }
        for s in dst.iter_mut() {
            *s = chip.generate_sample(self.sample_rate);
        }
    }
}

impl BufferView for Ay38910View {
    fn num_input_channels(&self) -> usize {
        self.num_channels
    }

    fn num_output_channels(&self) -> usize {
        0
    }

    fn read_input(&self, channel: usize, dst: &mut [f32]) -> usize {
        if channel >= self.num_channels {
            dst.fill(0.0);
            return dst.len();
        }
        // AY-3-8910 is mono — generate to all channels
        eprintln!(
            "AY38910 read_input: ch={channel} len={} reg0={} reg7={}",
            dst.len(),
            unsafe { (*self.register_buf)[0].load(Ordering::Relaxed) },
            unsafe { (*self.register_buf)[7].load(Ordering::Relaxed) },
        );
        unsafe {
            self.generate(dst);
        }
        dst.len()
    }

    fn write_output(&self, _channel: usize, _src: &[f32]) -> usize {
        0
    }
}

/// `IoBackend` + `IoControl` adapter for AY-3-8910 chip emulation.
///
/// Does NOT produce audio via I/O — the audio is generated on-the-fly
/// by [`Ay38910View`] returned from [`create_view`](Ay38910Backend::create_view).
/// Register writes go through `as_control()?.write_data()`, stored in atomics
/// for cross-thread safety.
#[allow(dead_code)]
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

impl IoBackend for Ay38910Backend {
    fn create_view(&self) -> Arc<dyn BufferView> {
        Arc::new(Ay38910View {
            chip: self.chip.get(),
            register_buf: &self.register_buf,
            sample_rate: self.sample_rate,
            num_channels: 1, // AY-3-8910 is mono; LofiInput duplicates to stereo
        })
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
        view.read_input(0, &mut buf);
        // Unconfigured chip should produce near-silence
        let max = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!(max < 0.5, "unconfigured chip max={max}");

        let mut active = [0u8; 16];
        active[0] = 23;
        active[1] = 1;
        active[7] = 0b11111110;
        active[8] = 10;
        let n = backend.as_control().unwrap().write_data(&active);
        assert_eq!(n, 16);

        view.read_input(0, &mut buf);
        let max_after = buf.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        // With registers configured, should produce some signal
        assert!(max_after > 0.001, "configured chip should produce signal");
    }
}
