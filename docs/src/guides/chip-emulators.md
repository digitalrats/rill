# Chip Emulators

Rill provides vintage sound chip emulation through a unified architecture:
`Chip` → `Backend` → `LofiInput` → `Output`. This guide covers the AY-3-8910
and NES 2A03 APU emulators in `rill-lofi`.

## Architecture

Every chip emulator follows the same three-layer model:

```
┌──────────────┐    ┌────────────────────┐    ┌───────────────────┐
│  Ay38910Chip │    │  Ay38910Backend    │    │  LofiInput<f32,N> │
│  pure logic  │───►│  IoBackend<f32>    │───►│  Source node      │
│  testable    │    │  + IoControl       │    │  lofi processing  │
└──────────────┘    └────────────────────┘    └───────────────────┘
```

### 1. Chip (pure logic)

Contains only the chip's digital model — registers, tone generators, noise,
envelopes. No audio I/O, no graph integration, no lofi processing. Testable
in isolation.

```rust
use rill_adrift::lofi::Ay38910Chip;

let mut chip = Ay38910Chip::new(1_750_000.0); // 1.75 MHz clock
chip.write_register(0, 0xF8); // tone period low
chip.write_register(1, 0x01); // tone period high → 0x1F8 ≈ 440 Hz
chip.write_register(8, 0x0F); // volume 15
chip.write_register(7, 0x3E); // mixer: Ch A tone on, noise off

let sample = chip.generate_sample(44100.0);
```

### 2. Backend (IoBackend + IoControl)

Wraps the chip as an `IoBackend<f32>`. `read()` generates audio. Chip registers
are written through `IoControl::write_data()` using atomic stores for cross-thread
safety.

```rust
use rill_adrift::lofi::Ay38910Backend;

let backend = Ay38910Backend::new(1_750_000.0, 44100.0);

// Write registers through IoControl
let ctrl = backend.as_control().unwrap();
let mut regs = [0u8; 16];
regs[0] = 0xF8; regs[1] = 0x01; // tone period
regs[8] = 15;                    // volume
regs[7] = 0x3E;                  // mixer
ctrl.write_data(&regs);

// Read audio
let mut buf = [0.0f32; 256];
backend.read(&mut [&mut buf[..]]);
```

### 3. LofiInput (Source node)

Wraps any `IoBackend<f32>` and applies vintage degradation: 8-bit bitcrushing,
noise floor, DAC nonlinearity, delay line. Configurable via `set_parameter`.

```rust
use rill_adrift::lofi::{Ay38910Backend, ClassicSystem, LofiConfig, LofiInput};

let mut lofi = LofiInput::<f32, 256>::new(
    LofiConfig::for_system(ClassicSystem::Custom {
        bit_depth: 8,
        sample_rate: 44100.0,
        nonlinear: false,
        noise_floor: -48.0,
    })
);
lofi.set_backend(Box::new(Ay38910Backend::new(1_750_000.0, 44100.0)));

// Send register writes to the backend
let regs: [u8; 16] = [/* ... */];
lofi.write_to_backend(&regs);
```

## Full example: AY-3-8910 chiptune player

See `rill-adrift/examples/chiptune.rs` for a complete working example.
The structure:

1. **ChiptuneSource** — owns `LofiInput<f32, BUF>` which wraps `Ay38910Backend`
2. **step()** — sequencer logic: computes note frequencies, writes register
   state via `lofi.write_to_backend(&regs)`
3. **generate()** — delegates to `lofi.generate()` which calls
   `backend.read()` → lofi processing → output ports

```rust
struct ChiptuneSource<const N: usize> {
    regs: [u8; 16],
    // sequencer state...
    lofi: LofiInput<f32, N>,
}

impl<const N: usize> Source<f32, N> for ChiptuneSource<N> {
    fn generate(&mut self, clock: &ClockTick, ctrl: &[f32], clk: &[ClockTick]) -> ProcessResult<()> {
        self.step();                              // write registers
        self.lofi.generate(clock, ctrl, clk)       // generate + lofi
    }
}
```

## NES 2A03 APU

The NES emulator follows the same pattern with `NesChip` + `NesBackend`.
NES registers are memory-mapped at `$4000–$4015` (22 bytes):

```rust
use rill_adrift::lofi::{NesBackend, LofiInput, ClassicSystem, LofiConfig};

let backend = NesBackend::new(44100.0);
let mut lofi = LofiInput::<f32, 256>::new(
    LofiConfig::for_system(ClassicSystem::Nes)
);
lofi.set_backend(Box::new(backend));

// NES registers: 22 bytes ($4000–$4015)
let mut regs = [0u8; 22];
regs[0] = 0x9F; // pulse1: duty 50%, volume 15
regs[2] = 0x00; // period low
regs[3] = 0x01; // period high → 0x100
regs[21] = 0x01; // enable pulse1
lofi.write_to_backend(&regs);
```

## Available chips

| Chip | Structs | Registers | Features |
|------|---------|-----------|----------|
| AY-3-8910 | `Ay38910Chip`, `Ay38910Backend` | 16 × 8-bit | 3 tone channels, noise, envelope |
| NES 2A03 | `NesChip`, `NesBackend` | 22 × 8-bit ($4000–$4015) | 2 pulse + sweep, triangle, noise, DPCM |

## IoControl trait

The `IoControl` trait in `rill-core::io` provides a uniform interface for
sending register data to chip backends:

```rust
pub trait IoControl: Send + Sync {
    fn write_data(&self, data: &[u8]) -> usize;
}
```

`IoBackend` has a default `as_control() -> Option<&dyn IoControl>` method.
Hardware backends return `None`. Chip backends return `Some(self)`.

## Lofi processing

`LofiInput` exposes these parameters via `set_parameter`:

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `enable_bitcrush` | Bool | true | 8/12-bit quantization |
| `enable_noise` | Bool | true | Vintage noise floor |
| `dry_wet` | Float | 1.0 | Wet/dry mix |
| `output_gain` | Float | 1.0 | Output gain (0.0–4.0) |

The noise floor uses proper dB-to-linear conversion (`10^(dB/20)`),
not a naive division by 100.
