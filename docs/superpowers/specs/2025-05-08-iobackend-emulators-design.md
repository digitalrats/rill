# IoBackend-based emulators + LofiInput

## Overview

Unify vintage chip emulators (AY-3-8910, NES APU, SID, ...) and real audio
hardware (PipeWire, ALSA, CPAL, JACK) under the `IoBackend<T>` trait.
`IoControl` trait provides chip-specific register access independently.

`LofiInput` wraps any `IoBackend` and applies vintage processing
(bitcrush, noise, DAC, delay) to its audio output. Chip control goes through
`as_control()` on `IoBackend`.

## Scope

| Crate | Changes |
|-------|---------|
| `rill-core` | Add `IoControl` trait, `as_control()` default method on `IoBackend` |
| `rill-lofi` | New: `Ay38910Chip`, `Ay38910Backend`, `LofiInput` |
| `rill-adrift` | Update chiptune example |

**`IoBackend<T>`, `IoBackendPtr`, `Input`, `Output`, `Graph`, `BackendFactory` — unchanged.**

## Architecture

```
 ┌────────────────┐       ┌─────────────────────────────────┐
 │ Ay38910Chip    │       │ LofiInput<T, BUF_SIZE>          │
 │ pure chip logic│       │ impl Source<T, BUF_SIZE>        │
 │ write_register │       │                                  │
 │ generate_sample│       │  ┌───────────────────────────┐  │
 └───────┬────────┘       │  │ Box<dyn IoBackend<T>>     │  │
         │                │  │ LofiProcessor<BUF_SIZE>   │  │
         ▼                │  │ UnsafeCell<...>           │  │
 ┌────────────────────┐   │  └───────────────────────────┘  │
 │ Ay38910Backend     │   │                                  │
 │ IoBackend<f32>     │   │  generate():                     │
 │ + IoControl        │   │    1. backend.read(bufs)          │
 │                    │   │    2. lofi.process_sample()       │
 │ read() -> f32      │   │    3. copy to output ports       │
 │ as_control() -> Io │   │                                  │
 │ write_data(regs)   │   │  write_to_backend(regs):         │
 └────────────────────┘   │    backend.as_control()          │
         │                │      ?.write_data(regs)          │
         │                └──────────────┬──────────────────┘
         any IoBackend ─────────────────┘
                         │
                         ▼
                    Output / Sink
```

## Key design decisions

1. **`IoBackend<T>` stays unchanged.** No `TIn`/`TOut` split. Chip register writes go
   through a separate `IoControl` trait, keeping audio data and control data orthogonal.

2. **`IoControl` trait for chip register writes.** `IoBackend` gets a default
   `as_control() -> Option<&dyn IoControl>` method. Hardware backends return `None`.
   Emulator backends return `Some(self)`.

3. **Chip control via `IoControl::write_data()`.** `LofiInput` accesses chip registers
   through `backend.as_control()?.write_data(data)`. `Ay38910Backend` stores registers
   in atomics for cross-thread safety.

4. **Only `rill-lofi` + `rill-core` modified.** No changes to `rill-io`, `rill-graph`,
   or any hardware backend.

## New types

### `IoControl` trait (rill-core)

```rust
/// Control interface for devices that accept operational data
/// separate from the audio stream (e.g. chip register writes).
pub trait IoControl: Send + Sync {
    /// Write control data. Interpretation is device-specific.
    /// For AY-3-8910: `data` contains 16 register values.
    fn write_data(&self, data: &[u8]) -> usize;
}
```

Add default method to `IoBackend`:
```rust
pub trait IoBackend<T: Scalar>: Send + Sync {
    // ... existing methods unchanged ...

    /// Returns a control interface if this backend supports runtime
    /// register/parameter writes. Returns `None` by default.
    fn as_control(&self) -> Option<&dyn IoControl> { None }
}
```

### `Ay38910Chip` (rill-lofi)

```rust
pub struct Ay38910Chip { /* channels, noise, envelope, mixer, registers */ }
impl Ay38910Chip {
    pub fn new(chip_clock: f32) -> Self;
    pub fn write_register(&mut self, reg: usize, value: u8);
    pub fn read_register(&self, reg: usize) -> u8;
    pub fn generate_sample(&mut self, sample_rate: f32) -> f32;
    pub fn reset(&mut self);
}
```

### `Ay38910Backend` (rill-lofi)

`IoBackend<f32> + IoControl`. Atomic registers for cross-thread writes.

```rust
pub struct Ay38910Backend {
    chip: UnsafeCell<Ay38910Chip>,
    sample_rate: f32,
    register_buf: [AtomicU8; 16],
}

impl IoBackend<f32> for Ay38910Backend {
    fn read(&self, channels: &mut [&mut [f32]]) -> usize { /* generate */ }
    fn write(&self, _channels: &[&[f32]]) -> usize { 0 }
    fn run(&self, _running: Arc<AtomicBool>) -> IoResult<()> { Ok(()) }
    fn stop(&self) -> IoResult<()> { Ok(()) }
    fn set_process_callback(&self, _cb: Box<dyn Fn()>) {}
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
```

### `LofiInput<T, BUF_SIZE>` (rill-lofi)

```rust
pub struct LofiInput<T: Transcendental, const BUF_SIZE: usize> {
    backend: Option<Box<dyn IoBackend<T>>>,
    lofi: LofiProcessor<BUF_SIZE>,
    // ...
}

impl LofiInput {
    pub fn write_to_backend(&self, data: &[u8]) -> usize {
        self.backend.as_ref()
            .and_then(|b| b.as_control())
            .map(|c| c.write_data(data))
            .unwrap_or(0)
    }
}
```

## Transition plan

1. Add `IoControl` trait + `as_control()` to `IoBackend` in rill-core
2. Extract `Ay38910Chip` from `Ay38910Emulator` in rill-lofi
3. Implement `Ay38910Backend` (`IoBackend<f32>` + `IoControl`) in rill-lofi
4. Deprecate old `Ay38910Emulator` (delegates to chip internally)
5. Implement `LofiInput<T, BUF_SIZE>` in rill-lofi
6. Update chiptune example in rill-adrift
7. Future: NES, Akai S900 under same pattern
