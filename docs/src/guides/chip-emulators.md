# Chip Emulators

Rill provides vintage sound chip emulation through a three-layer architecture:
`Chip` → `Backend` → `LofiInput`. This guide covers the AY-3-8910 emulator in
`rill-lofi`.

## Architecture

Every chip emulator follows the same model:

```
┌──────────────┐    ┌────────────────────┐    ┌───────────────────┐
│  Ay38910Chip │    │  Ay38910Backend    │    │  LofiInput<f32,N> │
│  pure logic  │───►│  IoBackend<f32>    │───►│  Source node      │
│  testable    │    │  + IoControl       │    │  lofi processing  │
└──────────────┘    └────────────────────┘    └───────────────────┘
```

### 1. Chip (`Ay38910Chip`) — pure logic

Contains only the chip's digital model — registers, tone generators, noise LFSR,
envelope. No audio I/O, no graph integration, no lofi processing. Directly testable.

**AY-3-8910 register map (16 × 8-bit):**

| R# | Name | Bits | Description |
|----|------|------|-------------|
| R0–R1 | Tone A period | 12 | `f = 1.75 MHz / (16 × TP)` |
| R2–R3 | Tone B period | 12 | |
| R4–R5 | Tone C period | 12 | |
| R6 | Noise period | 5 | `f = 1.75 MHz / (16 × NP)` |
| R7 | Mixer | 8 | Bits 0–2: tone A/B/C, 3–5: noise A/B/C (0=ON) |
| R8–R10 | Volume A/B/C | 5 | Bit 4: envelope mode, bits 0–3: 0–15 |
| R11–R12 | Envelope period | 16 | `f = 1.75 MHz / (256 × EP)` |
| R13 | Envelope shape | 4 | Continue, Attack, Alternate, Hold |
| R14–R15 | I/O port A/B | 8 | Not implemented (audio only) |

```rust
use rill_adrift::lofi::Ay38910Chip;

let mut chip = Ay38910Chip::new(1_750_000.0); // 1.75 MHz clock
chip.write_register(0, 0x17); // tone period low
chip.write_register(1, 0x01); // tone period high → 279 → ~392 Hz
chip.write_register(8, 0x0A); // volume 10 (fixed)
chip.write_register(7, 0x38); // mixer: Ch A tone+noise ON, B/C tone ON

let sample = chip.generate_sample(44100.0);
```

### 2. Backend (`Ay38910Backend`) — IoBackend + IoControl

Wraps the chip as an `IoBackend<f32>`. `read()` loads register values from atomics
and generates audio sample-by-sample. Register writes go through `IoControl::write_data()`
using atomic stores for cross-thread safety (control thread → audio thread).

```rust
use rill_adrift::lofi::Ay38910Backend;

let backend = Ay38910Backend::new(1_750_000.0, 44100.0);
let ctrl = backend.as_control().unwrap();
let regs = [0x17, 0x01, 0, 0, 0, 0, 0, 0x38, 0x0A, 0, 0, 0, 0, 0, 0, 0];
ctrl.write_data(&regs);           // writes all 16 registers atomically

let mut buf = [0.0f32; 256];
backend.read(&mut [&mut buf[..]]);  // generates 256 samples
```

### 3. LofiInput — Source node in the graph

`LofiInput` wraps any `IoBackend<f32>` and applies vintage degradation: bitcrushing,
noise floor, DAC nonlinearity, delay. Configured at construction and runtime-tunable
via `set_parameter`.

In a typical graph (e.g., `chiptune.rs`): the sequencer (via Servo + Automaton) sends
register bytes to `LofiInput.set_parameter("io_write", ParamValue::Bytes(regs))`,
which calls `backend.write_data(regs)` on the AY backend.

```
[SequencerAutomaton] → [Servo] → SetParameter("io_write", regs)
                                      │
┌─────────────────────────────────────┘
▼
Graph tick: actor.drain()
  → LofiInput.set_parameter("io_write", regs)  // writes registers
  → LofiInput.generate()                        // reads backend, lofi processing
  → propagate → Output                          // audio to device
```

## Full example: AY-3-8910 chiptune player

See `rill-adrift/examples/chiptune.rs` — uses `ModularSystemDef` with `SequencerAutomaton`,
table-based Servo, and `LofiInput` + `Ay38910Backend`.

See `rill-adrift/examples/chiptune_stc.rs` — loads `.stc` tracker files, demonstrates
`ModuleFactory` for custom rack modules.

## Lofi processing

`LofiInput` processes each sample through this chain:

```
input → bitcrush → sample-rate reduction → noise → DAC emulation → delay → dry/wet mix → output_gain
```

Configurable via `set_parameter`:

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `enable_bitcrush` | Bool | true | Quantization to `bit_depth` bits |
| `enable_noise` | Bool | true | Vintage noise floor (dB → linear) |
| `enable_sr_reduction` | Bool | true | Sample-rate decimation |
| `dry_wet` | Float | 1.0 | Wet/dry mix (0.0 = dry, 1.0 = fully processed) |
| `output_gain` | Float | 1.0 | Output gain (0.0–4.0) |

For `ClassicSystem::Custom`, three parameters are set at construction via `LofiConfig`:

| Parameter | Example | Description |
|-----------|---------|-------------|
| `bit_depth` | 8 | Quantization bit depth |
| `nonlinear` | false | Non-linear encoding (dead code for Custom) |
| `noise_floor` | -48.0 | Noise floor in dB |

**Important:** The lofi chain with default settings (bitcrush=8, noise=-48dB, DAC emulation)
aggressively degrades the signal. For a clean chiptune tone, tune the parameters — higher
`bit_depth`, lower `noise_floor`, or bypass via `enable_bitcrush=false` + `enable_noise=false`.

## Known limitations

### Emulator accuracy

The AY-3-8910 emulator is a **functional model**, not a cycle-accurate replica. It produces
recognisable AY-like audio suitable for music playback, but differs from real hardware in
these ways:

| Aspect | Current behaviour | Real AY-3-8910 |
|--------|-------------------|----------------|
| **Output sampling** | 1 sample per `generate_sample()` call | Continuous analog output with infinite bandwidth |
| **Anti-aliasing** | None | Implicit in analog stage (amplifier bandpass) |
| **Noise LFSR** | 17-bit, output = bit 0, polynomial x^17+x^14+1 | Same LFSR, but output filtered by analog stage |
| **Envelope** | 4-bit mode, 16-bit period, linear ramp | Same, but real chip has minor non-linearities |
| **Register changes** | Applied at start of next `generate_sample()` block (up to 1/sample_rate delay) | Applied at next tone period boundary |
| **I/O ports (R14–R15)** | Not implemented | Bidirectional 8-bit GPIO |
| **YM2149 compatibility** | Not implemented | YM variant has `/2` clock divider, minor differences |

### Timing accuracy

- **Tone frequency:** Formula correct (`f_clock / (16 × TP)`), phase accumulator preserves
  fractional remainder → no long-term drift. Frequency accuracy ≈ 0.05% at 44100 Hz.
- **Envelope timing:** Formula correct (`f_clock / (256 × EP)`, fixed in 0.5.0-beta.5).
  Envelope steps are discrete (16 per cycle), exact transition times depend on sample rate.
- **Noise timing:** Formula correct (`f_clock / (16 × NP)`). Output bit sampled at audio rate
  without bandlimiting → aliasing folds high-frequency noise into audible range.
- **STC interrupt rate:** 48.828125 Hz (`f_clock / 35840`), approximated via `step_ms()`
  with floating-point accumulator → sub-microsecond jitter.

### Phase relationship between tone, noise, and envelope

All three generators run from the same master clock but are sampled independently in
`generate_sample()`:

1. Tone phase is advanced
2. Noise and envelope states are **read** (from their previous state)
3. Channel outputs computed
4. Noise phase advanced (`update_noise`)
5. Envelope phase advanced (`update_envelope`)

This means noise and envelope are always **one sample behind** tone in the same block.
At 44100 Hz this is 22.7 µs — inaudible, but means phase correlation measurements will
differ from hardware by one sample period.

## Available chips

| Chip | Structs | Registers | Features |
|------|---------|-----------|----------|
| AY-3-8910 | `Ay38910Chip`, `Ay38910Backend` | 16 × 8-bit | 3 tone channels, noise LFSR, envelope |
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
