# NES APU Sweep Unit

## Overview

Full hardware sweep unit for NES 2A03 APU pulse channels (pulse 1 and pulse 2).
Implements period sweep with configurable divider, direction, and shift amount.

## Scope

- `NesSweepUnit` struct in `rill-lofi/src/emulators/nes_chip.rs`
- Two instances inside `NesChip` (one per pulse channel)
- Register parsing from $4001 (pulse 1) and $4005 (pulse 2)
- Sweep clock driven by `generate_sample` at 120 Hz

## Design

```rust
struct NesSweepUnit {
    enabled: bool,
    reload: bool,            // true after register write
    divider_period: u8,      // 0-7, written to bits 6-4
    divider_counter: u8,     // current countdown
    negate: bool,            // direction: false=increase period
    shift: u8,               // amount to shift period
    target_period: u16,      // the channel's raw 11-bit period
}
```

## Algorithm

On sweep clock tick (120 Hz):
1. If `reload`: reset `divider_counter = divider_period`, clear `reload`
2. If `divider_counter == 0`:
   - `delta = target_period >> shift`
   - `new = negate ? target_period - delta - 1 : target_period + delta`
   - If `new < 8 || new > 0x7FF`: mute channel (set enabled=false internally)
   - Else: `target_period = new`
3. If `divider_counter > 0`: decrement

Register write ($4001/$4005):
- `enabled = bit7`
- `divider_period = bits[6:4]`
- `negate = bit3`
- `shift = bits[2:0]`
- `reload = true`

## Integration

`NesChip::write_registers` parses $4001/$4005 into `sweep1`/`sweep2`.
`generate_sample` clocks both sweep units at 120 Hz and updates
`pulse1.frequency` / `pulse2.frequency` from `sweep.target_period`.
