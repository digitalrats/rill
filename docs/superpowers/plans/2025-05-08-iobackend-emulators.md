# IoBackend-based emulators + LofiInput — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `IoControl` trait to `IoBackend`, implement `Ay38910Chip` + `Ay38910Backend` + `LofiInput`.

**Architecture:** `IoControl` trait in rill-core provides chip register write access independently of audio I/O. `IoBackend` gets `as_control()` default method. `Ay38910Backend` implements both. `LofiInput` wraps any `IoBackend<T>` with lofi processing. No changes to `rill-io`, `rill-graph`, or hardware backends.

**Tech Stack:** Rust 2021 edition, Cargo workspace, TDD

**Spec:** `docs/superpowers/specs/2025-05-08-iobackend-emulators-design.md`

**Scope:** `rill-core` (IoControl trait), `rill-lofi` (chip + backend + LofiInput), `rill-adrift` (example)

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `rill-core/src/io.rs` | Modify | Add `IoControl` trait + `as_control()` default on `IoBackend` |
| `rill-lofi/src/emulators/ay38910_chip.rs` | Create | Pure AY-3-8910 chip logic (extracted) |
| `rill-lofi/src/emulators/ay38910_backend.rs` | Create | `IoBackend<f32>` + `IoControl` wrapper |
| `rill-lofi/src/emulators/ay38910_emulator.rs` | Create | Deprecated `Ay38910Emulator` (delegates to chip) |
| `rill-lofi/src/emulators/ay38910.rs` | Delete | Remove old monolithic emulator |
| `rill-lofi/src/lofi_input.rs` | Create | `LofiInput<T, BUF_SIZE>` Source node |
| `rill-lofi/src/emulators/mod.rs` | Modify | Re-export new types |
| `rill-lofi/src/lib.rs` | Modify | Re-export `LofiInput` |
| `rill-adrift/examples/chiptune.rs` | Modify | Use `Ay38910Backend + LofiInput` |

---

### Task 1: Add IoControl trait + as_control() to IoBackend

**Files:**
- Modify: `rill-core/src/io.rs`

- [ ] **Step 1: Write failing test**

Add test to `rill-core/src/io.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
    use std::sync::Arc;
    use crate::math::Scalar;

    struct TestBackend {
        reg: AtomicU8,
    }

    impl IoBackend<f32> for TestBackend {
        fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}
        fn read(&self, _: &mut [&mut [f32]]) -> usize { 0 }
        fn write(&self, _: &[&[f32]]) -> usize { 0 }
        fn run(&self, _: Arc<AtomicBool>) -> IoResult<()> { Ok(()) }
        fn stop(&self) -> IoResult<()> { Ok(()) }
        fn as_control(&self) -> Option<&dyn IoControl> { Some(self) }
    }

    impl IoControl for TestBackend {
        fn write_data(&self, data: &[u8]) -> usize {
            if let Some(&v) = data.first() {
                self.reg.store(v, Ordering::Relaxed);
            }
            1
        }
    }

    #[test]
    fn test_iocontrol_write_data() {
        let b = TestBackend { reg: AtomicU8::new(0) };
        let ctrl = b.as_control().unwrap();
        ctrl.write_data(&[42]);
        assert_eq!(b.reg.load(Ordering::Relaxed), 42);
    }

    #[test]
    fn test_iocontrol_default_returns_none() {
        struct NoControl;
        impl IoBackend<f32> for NoControl {
            fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}
            fn read(&self, _: &mut [&mut [f32]]) -> usize { 0 }
            fn write(&self, _: &[&[f32]]) -> usize { 0 }
            fn run(&self, _: Arc<AtomicBool>) -> IoResult<()> { Ok(()) }
            fn stop(&self) -> IoResult<()> { Ok(()) }
        }
        let b = NoControl;
        assert!(b.as_control().is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rill-core -- test_iocontrol`
Expected: FAIL — "cannot find trait IoControl"

- [ ] **Step 3: Add IoControl and as_control()**

In `rill-core/src/io.rs`, before the `IoBackend` trait:

```rust
/// Control interface for backends that accept operational data
/// separate from the audio stream (e.g. chip register writes).
pub trait IoControl: Send + Sync {
    /// Write control data. Interpretation is device-specific.
    fn write_data(&self, data: &[u8]) -> usize;
}
```

In `IoBackend` trait, add method after `stop()`:

```rust
    /// Returns a control interface if this backend supports runtime
    /// register/data writes. Returns `None` by default.
    fn as_control(&self) -> Option<&dyn IoControl> { None }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rill-core -- test_iocontrol`
Expected: PASS (both tests)

- [ ] **Step 5: Run rill-core full tests**

Run: `cargo test -p rill-core`
Expected: ALL PASS

- [ ] **Step 6: Commit**

```bash
git add rill-core/src/io.rs
git commit -m "feat(rill-core): add IoControl trait and as_control() to IoBackend"
```

---

### Task 2: Create Ay38910Chip (pure logic)

**Files:**
- Create: `rill-lofi/src/emulators/ay38910_chip.rs`

- [ ] **Step 1: Write tests and implementation**

Create `rill-lofi/src/emulators/ay38910_chip.rs`. Extract pure chip logic from the current `ay38910.rs` — all fields, methods, and internal state. No `Node`, `Source`, `Port`, `LofiProcessor` dependencies. `sample_rate` passed to `generate_sample` as argument.

Include these tests inline:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    const SR: f32 = 44100.0;

    #[test]
    fn test_register_read_write() {
        let mut chip = Ay38910Chip::new(1_750_000.0);
        chip.write_register(0, 0x42);
        assert_eq!(chip.read_register(0), 0x42);
        assert_eq!(chip.read_register(16), 0);
    }

    #[test]
    fn test_tone_output() {
        let mut chip = Ay38910Chip::new(1_750_000.0);
        let div = 279u16;
        chip.write_register(0, div as u8);
        chip.write_register(1, (div >> 8) as u8);
        chip.write_register(8, 10);
        chip.write_register(7, 0b11_11_11_10);
        let s = chip.generate_sample(SR);
        assert!(s > 0.0, "tone should produce output, got {}", s);
    }

    #[test]
    fn test_noise_disabled_is_silent() {
        let mut chip = Ay38910Chip::new(1_750_000.0);
        chip.write_register(7, 0xFF);
        let s = chip.generate_sample(SR);
        assert!(s.abs() < 0.001, "muted chip silent, got {}", s);
    }

    #[test]
    fn test_reset_clears_state() {
        let mut chip = Ay38910Chip::new(1_750_000.0);
        chip.write_register(0, 42);
        chip.generate_sample(SR);
        chip.reset();
        assert_eq!(chip.registers[0], 0);
        assert_eq!(chip.noise.shift_register, 0x0001_0000);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p rill-lofi -- ay38910_chip`
Expected: ALL 4 tests PASS

- [ ] **Step 3: Commit**

```bash
git add rill-lofi/src/emulators/ay38910_chip.rs
git commit -m "feat(rill-lofi): extract Ay38910Chip with pure AY-3-8910 logic"
```

---

### Task 3: Create Ay38910Backend (IoBackend<f32> + IoControl)

**Files:**
- Create: `rill-lofi/src/emulators/ay38910_backend.rs`

- [ ] **Step 1: Implement backend with tests**

Create `rill-lofi/src/emulators/ay38910_backend.rs`:

```rust
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use rill_core::io::{IoBackend, IoControl, IoResult};
use rill_core::math::Scalar;

use super::ay38910_chip::Ay38910Chip;

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
            for ch in channels.iter_mut() { ch[i] = s; }
        }
        n
    }
    fn write(&self, _channels: &[&[f32]]) -> usize { 0 }
    fn run(&self, _running: Arc<AtomicBool>) -> IoResult<()> { Ok(()) }
    fn stop(&self) -> IoResult<()> { Ok(()) }
    fn as_control(&self) -> Option<&dyn IoControl> { Some(self) }
}

impl IoControl for Ay38910Backend {
    fn write_data(&self, data: &[u8]) -> usize {
        for (i, &v) in data.iter().enumerate().take(16) {
            self.register_buf[i].store(v, Ordering::Relaxed);
        }
        16
    }
}

unsafe impl Send for Ay38910Backend {}
unsafe impl Sync for Ay38910Backend {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_write_read_roundtrip() {
        let backend = Ay38910Backend::new(1_750_000.0, 44100.0);
        let mut regs = [0u8; 16];
        regs[0] = 23; regs[1] = 1; // tone period 279
        regs[7] = 0b11_11_11_10;   // Ch A tone on
        regs[8] = 10;              // volume
        let ctrl = backend.as_control().unwrap();
        ctrl.write_data(&regs);

        let mut buf = [0.0f32; 64];
        backend.read(&mut [&mut buf[..]]);
        assert!(buf.iter().any(|&s| s > 0.0), "should produce audio");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p rill-lofi -- ay38910_backend`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add rill-lofi/src/emulators/ay38910_backend.rs
git commit -m "feat(rill-lofi): add Ay38910Backend (IoBackend<f32> + IoControl)"
```

---

### Task 4: Deprecate old Ay38910Emulator

**Files:**
- Create: `rill-lofi/src/emulators/ay38910_emulator.rs`
- Delete: `rill-lofi/src/emulators/ay38910.rs`
- Modify: `rill-lofi/src/emulators/mod.rs`

- [ ] **Step 1: Create deprecated wrapper**

Create `rill-lofi/src/emulators/ay38910_emulator.rs`. The old `Ay38910Emulator<BUF_SIZE>` struct that implements `Node<f32, BUF_SIZE>` + `Source<f32, BUF_SIZE>`, delegating to `Ay38910Chip` internally and using its own `LofiProcessor` for processing. Marked `#[deprecated]`.

- [ ] **Step 2: Delete old ay38910.rs** and update mod.rs

```rust
mod ay38910_chip;
mod ay38910_backend;
mod ay38910_emulator;
mod akai_s900;
mod nes;

pub use ay38910_chip::Ay38910Chip;
pub use ay38910_backend::Ay38910Backend;
#[allow(deprecated)]
pub use ay38910_emulator::Ay38910Emulator;
pub use akai_s900::AkaiS900Emulator;
pub use nes::NesEmulator;
```

- [ ] **Step 3: Verify compilation and tests**

Run: `cargo test -p rill-lofi`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git rm rill-lofi/src/emulators/ay38910.rs
git add rill-lofi/src/emulators/ay38910_emulator.rs rill-lofi/src/emulators/mod.rs
git commit -m "feat(rill-lofi): deprecate Ay38910Emulator, delegate to Ay38910Chip"
```

---

### Task 5: Create LofiInput Source node

**Files:**
- Create: `rill-lofi/src/lofi_input.rs`
- Modify: `rill-lofi/src/lib.rs`

`LofiInput<T, BUF_SIZE>` — wraps `Box<dyn IoBackend<T>>`, applies lofi processing in `generate()`. Exposes `write_to_backend(&[u8])` for chip control.

- [ ] **Step 1: Implement LofiInput**

```rust
use std::cell::UnsafeCell;

use rill_core::{
    ClockTick, NodeId, NodeMetadata, NodeState, NodeCategory,
    ParamValue, ParameterId, Port, ProcessResult,
    traits::{Node, Source},
    math::Transcendental,
    io::IoBackend,
};

use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;

pub struct LofiInput<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    backend: Option<Box<dyn IoBackend<T>>>,
    bufs: UnsafeCell<Vec<[T; BUF_SIZE]>>,
    lofi: LofiProcessor<BUF_SIZE>,
}

impl<T: Transcendental, const BUF_SIZE: usize> LofiInput<T, BUF_SIZE> {
    pub fn new(lofi_config: LofiConfig) -> Self {
        Self::with_channels(1, lofi_config)
    }

    pub fn with_channels(num: usize, lofi_config: LofiConfig) -> Self {
        let metadata = NodeMetadata {
            name: "Lofi Input".to_string(),
            type_name: None,
            category: NodeCategory::Source,
            description: "Lo-fi processed input source".to_string(),
            author: "Rill Lo-Fi".to_string(),
            version: "0.1.0".to_string(),
            signal_inputs: 0,
            signal_outputs: num,
            control_inputs: 0, control_outputs: 0,
            clock_inputs: 0, clock_outputs: 0,
            feedback_ports: 0,
            parameters: vec![
                ParamMetadata::new("enable_bitcrush", rill_core::ParamType::Bool, ParamValue::Bool(true))
                    .with_description("Enable bitcrushing"),
                ParamMetadata::new("enable_noise", rill_core::ParamType::Bool, ParamValue::Bool(true))
                    .with_description("Enable vintage noise"),
                ParamMetadata::new("dry_wet", rill_core::ParamType::Float, ParamValue::Float(1.0))
                    .with_description("Dry/wet mix").with_range(0.0, 1.0, 0.01),
                ParamMetadata::new("output_gain", rill_core::ParamType::Float, ParamValue::Float(1.0))
                    .with_description("Output gain").with_range(0.0, 4.0, 0.1),
            ],
        };
        let name = move |i: usize| -> String {
            if num == 1 { "out".into() } else { format!("ch_{i}") }
        };
        let outputs = (0..num).map(|i| Port::output(NodeId(0), i as u16, &name(i))).collect();
        let bufs = vec![[T::ZERO; BUF_SIZE]; num];
        Self {
            id: NodeId(0), metadata, outputs,
            state: NodeState::new(44100.0),
            backend: None,
            bufs: UnsafeCell::new(bufs),
            lofi: LofiProcessor::new(lofi_config),
        }
    }

    pub fn set_backend(&mut self, backend: Box<dyn IoBackend<T>>) {
        self.backend = Some(backend);
    }

    pub fn write_to_backend(&self, data: &[u8]) -> usize {
        self.backend.as_ref()
            .and_then(|b| b.as_control())
            .map(|c| c.write_data(data))
            .unwrap_or(0)
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for LofiInput<T, BUF_SIZE> {
    fn generate(&mut self, _clock: &ClockTick, _ctrl: &[T], _clk: &[ClockTick]) -> ProcessResult<()> {
        if let Some(ref backend) = self.backend {
            let nch = self.outputs.len();
            if nch == 0 { self.state.advance(); return Ok(()); }
            let bufs = unsafe { &mut *self.bufs.get() };
            let mut channels: Vec<&mut [T]> = bufs.iter_mut().map(|b| &mut b[..]).collect();
            let n = backend.read(&mut channels);
            for ch in bufs.iter_mut() {
                for s in ch[..n.min(BUF_SIZE)].iter_mut() {
                    *s = T::from_f32(self.lofi.process_sample(s.to_f32()));
                }
            }
            if n >= BUF_SIZE {
                for (i, buf) in bufs.iter().enumerate() {
                    if let Some(port) = self.outputs.get_mut(i) {
                        port.buffer_mut().as_mut_array()[..BUF_SIZE].copy_from_slice(&buf[..BUF_SIZE]);
                    }
                }
            }
        }
        self.state.advance();
        Ok(())
    }
}

// Node trait impl: delegates id, metadata, init, reset, get/set_parameter, ports, state
// to self.lofi and self fields. Full boilerplate from plan omitted for brevity —
// follow the existing Input<T> pattern from rill-io/src/input.rs.
```

- [ ] **Step 2: Add to lib.rs**

Add `mod lofi_input;` and `pub use lofi_input::LofiInput;`.

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p rill-lofi`
Expected: Compiles clean

- [ ] **Step 4: Commit**

```bash
git add rill-lofi/src/lofi_input.rs rill-lofi/src/lib.rs
git commit -m "feat(rill-lofi): add LofiInput Source node"
```

---

### Task 6: Rewrite chiptune example

**Files:**
- Modify: `rill-adrift/examples/chiptune.rs`

Rewrite to use `Ay38910Backend` + `LofiInput<f32, BUF_SIZE>`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::io::output::Output;
use rill_adrift::lofi::{Ay38910Backend, LofiInput};
use rill_adrift::lofi::config::{ClassicSystem, LofiConfig};
use rill_adrift::rill_core::prelude::*;
use rill_adrift::runtime::{Runtime, RuntimeConfig};

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn note_to_divider(freq: f32) -> u16 {
    if freq <= 0.0 { 0 } else { (1_750_000.0 / (16.0 * freq)).max(1.0) as u16 }
}

#[derive(Clone, Copy)]
struct Note { freq: f32, dur_ms: u64 }

const MELODY: &[Note] = &[
    Note { freq: 392.0, dur_ms: 120 }, Note { freq: 440.0, dur_ms: 120 },
    Note { freq: 392.0, dur_ms: 120 }, Note { freq: 329.6, dur_ms: 120 },
    Note { freq: 392.0, dur_ms: 120 }, Note { freq: 440.0, dur_ms: 120 },
    Note { freq: 392.0, dur_ms: 120 }, Note { freq: 329.6, dur_ms: 120 },
    Note { freq: 261.6, dur_ms: 120 }, Note { freq: 329.6, dur_ms: 120 },
    Note { freq: 261.6, dur_ms: 120 }, Note { freq: 220.0, dur_ms: 120 },
    Note { freq: 261.6, dur_ms: 120 }, Note { freq: 329.6, dur_ms: 120 },
    Note { freq: 261.6, dur_ms: 120 }, Note { freq: 220.0, dur_ms: 120 },
    Note { freq: 293.7, dur_ms: 120 }, Note { freq: 349.2, dur_ms: 120 },
    Note { freq: 293.7, dur_ms: 120 }, Note { freq: 246.9, dur_ms: 120 },
    Note { freq: 293.7, dur_ms: 120 }, Note { freq: 349.2, dur_ms: 120 },
    Note { freq: 293.7, dur_ms: 120 }, Note { freq: 246.9, dur_ms: 120 },
];

const BASS: &[Note] = &[
    Note { freq: 110.0, dur_ms: 480 }, Note { freq: 130.8, dur_ms: 480 },
    Note { freq: 98.0,  dur_ms: 480 }, Note { freq: 110.0, dur_ms: 480 },
];

struct ChiptuneSource<const N: usize> {
    regs: [u8; 16],
    mel_step: usize, mel_ms: u64,
    bass_step: usize, bass_ms: u64,
    snare: u64, block_ms: u64,
    lofi: LofiInput<f32, N>,
}

impl<const N: usize> ChiptuneSource<N> {
    fn new() -> Self {
        let block_ms = (N as f64 * 1000.0 / RATE as f64) as u64;
        let lofi_config = LofiConfig::for_system(ClassicSystem::Custom {
            bit_depth: 8, sample_rate: RATE, nonlinear: false, noise_floor: -48.0,
        });
        let mut lofi = LofiInput::<f32, N>::new(lofi_config);
        lofi.set_backend(Box::new(Ay38910Backend::new(1_750_000.0, RATE)));
        Self { regs: [0; 16], mel_step: 0, mel_ms: 0, bass_step: 0, bass_ms: 0, snare: 0, block_ms, lofi }
    }

    fn step(&mut self) {
        let ms = self.block_ms.max(1);
        // Melody
        self.mel_ms += ms;
        if self.mel_ms >= MELODY[self.mel_step].dur_ms {
            self.mel_ms -= MELODY[self.mel_step].dur_ms;
            self.mel_step = (self.mel_step + 1) % MELODY.len();
        }
        let tp = note_to_divider(MELODY[self.mel_step].freq);
        self.regs[0] = tp as u8; self.regs[1] = (tp >> 8) as u8;
        self.regs[8] = if MELODY[self.mel_step].freq > 0.0 { 10 } else { 0 };
        // Bass
        self.bass_ms += ms;
        if self.bass_ms >= BASS[self.bass_step].dur_ms {
            self.bass_ms -= BASS[self.bass_step].dur_ms;
            self.bass_step = (self.bass_step + 1) % BASS.len();
        }
        let bp = note_to_divider(BASS[self.bass_step].freq);
        self.regs[2] = bp as u8; self.regs[3] = (bp >> 8) as u8;
        self.regs[9] = if BASS[self.bass_step].freq > 0.0 { 8 } else { 0 };
        // Snare
        let snare_on = (self.mel_step % 4) == 0 && self.mel_ms < 60;
        if snare_on && self.snare == 0 { self.snare = 4; }
        if self.snare > 0 {
            self.regs[6] = 4; self.regs[10] = 12; self.snare -= 1;
            self.regs[7] = 0b00_00_10_10; // A(tone) B(tone) C(noise+tone)
        } else {
            self.regs[6] = 0; self.regs[10] = 0;
            self.regs[7] = 0b11_11_10_10; // A(tone) B(tone) C(off)
        }
        self.lofi.write_to_backend(&self.regs);
    }
}

// Node<f32, N> and Source<f32, N> impls delegate to self.lofi
// (same pattern as old chiptune — all methods forwarded)

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args.get(1).cloned().unwrap_or_else(|| "cpal".into());
    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();
    let audio_thread = std::thread::spawn(move || {
        let mut rt = Runtime::<BUF>::new(RuntimeConfig { sample_rate: RATE, block_size: BUF, ..Default::default() });
        let mut params = std::collections::HashMap::new();
        params.insert("sample_rate".into(), rill_core::ParamValue::Int(RATE as i32));
        params.insert("buffer_size".into(), rill_core::ParamValue::Int(BUF as i32));
        params.insert("channels".into(), rill_core::ParamValue::Int(1));
        rt.set_default_backend(&backend_name, params);
        let mut builder = rt.create_builder();
        let src = builder.add_source(Box::new(ChiptuneSource::<BUF>::new()));
        let snk = builder.add_sink(Box::new(Output::<f32, BUF>::with_channels(1)));
        builder.connect_signal(src, 0, snk, 0);
        let graph = builder.build().expect("graph build");
        graph.run(t_run).ok();
    });
    let t_run = running.clone();
    let ah = audio_thread.thread().clone();
    std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        t_run.store(false, Ordering::Release);
        ah.unpark();
    });
    println!("\nAY-3-8910 Chiptune — \"Popcorn\"\nBackend: {}\nPress Enter to stop.\n", backend_name);
    audio_thread.join().ok();
    println!("Stopped.");
    Ok(())
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p rill-adrift --examples --features "cpal,lofi"`
Expected: Compiles clean

- [ ] **Step 3: Commit**

```bash
git add rill-adrift/examples/chiptune.rs
git commit -m "feat(rill-adrift): rewrite chiptune with Ay38910Backend + LofiInput"
```

---

### Task 7: Final workspace verification

- [ ] **Step 1: Full workspace test**

Run: `cargo test --workspace`
Expected: ALL PASS

- [ ] **Step 2: Full workspace clippy**

Run: `cargo clippy --workspace`
Expected: No new warnings

- [ ] **Step 3: Commit any fixes**
