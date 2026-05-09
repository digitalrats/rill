# NES APU Sweep Unit — Implementation Plan

## Tasks

### Task 1: Add `NesSweepUnit` struct to `nes_chip.rs`

Add before `NesChip` struct:

```rust
struct NesSweepUnit {
    enabled: bool,
    reload: bool,
    divider_period: u8,
    divider_counter: u8,
    negate: bool,
    shift: u8,
    target_period: u16,
}

impl NesSweepUnit {
    fn new() -> Self {
        Self {
            enabled: false,
            reload: false,
            divider_period: 0,
            divider_counter: 0,
            negate: false,
            shift: 0,
            target_period: 0,
        }
    }

    fn write_register(&mut self, value: u8) {
        self.enabled = (value & 0x80) != 0;
        self.divider_period = (value >> 4) & 0x07;
        self.negate = (value & 0x08) != 0;
        self.shift = value & 0x07;
        self.reload = true;
    }

    fn set_target_period(&mut self, period: u16) {
        self.target_period = period & 0x07FF;
    }

    /// Clock the sweep unit at ~120 Hz. Returns the new period
    /// or None if the channel should be silenced.
    fn clock(&mut self) -> Option<u16> {
        if self.divider_period == 0 && !self.reload {
            return Some(self.target_period);
        }
        if self.reload {
            self.divider_counter = self.divider_period;
            self.reload = false;
        }
        if self.divider_counter > 0 {
            self.divider_counter -= 1;
        }
        if self.divider_counter == 0 && self.divider_period > 0 {
            let delta = self.target_period >> self.shift as u32;
            let new = if self.negate && self.target_period >= delta + 1 {
                self.target_period - delta - 1
            } else if self.negate {
                0
            } else {
                self.target_period + delta
            };
            if new < 8 || new > 0x07FF {
                return None;
            }
            self.target_period = new;
        }
        Some(self.target_period)
    }
}
```

### Task 2: Add sweep units to `NesChip`

```rust
pub struct NesChip {
    pulse1: NesPulseChannel,
    pulse2: NesPulseChannel,
    triangle: NesTriangleChannel,
    noise: NesNoiseChannel,
    dpcm: NesDpcmChannel,
    sweep1: NesSweepUnit,
    sweep2: NesSweepUnit,
}
```

### Task 3: Parse sweep registers in `write_registers`

Add after pulse 1 period parsing:
```rust
self.sweep1.write_register(regs[1]);
self.sweep1.set_target_period(p1_period);
```

And after pulse 2:
```rust
self.sweep2.write_register(regs[5]);
self.sweep2.set_target_period(p2_period);
```

### Task 4: Clock sweep units in `generate_sample`

Add sweep clock tracking and application:
```rust
// Sweep clock at ~120 Hz
self.sweep_phase += 120.0 / sample_rate;
while self.sweep_phase >= 1.0 {
    self.sweep_phase -= 1.0;
    if let Some(period) = self.sweep1.clock() {
        self.pulse1.frequency = if period > 0 {
            1_789_773.0 / (16.0 * (period + 1) as f32)
        } else { 0.0 };
    } else {
        self.pulse1.enabled = false;
    }
    if let Some(period) = self.sweep2.clock() {
        self.pulse2.frequency = if period > 0 {
            1_789_773.0 / (16.0 * (period + 1) as f32)
        } else { 0.0 };
    } else {
        self.pulse2.enabled = false;
    }
}
```

### Task 5: Add sweep_phase to NesChip and reset

Add `sweep_phase: f32` field, initialize to 0.0, reset to 0.0 in `reset()`.

### Task 6: Add test

```rust
#[test]
fn test_sweep_unit_basic() {
    let mut unit = NesSweepUnit::new();
    unit.write_register(0x8F); // enabled, divider=4, negate, shift=7
    unit.set_target_period(0x100);
    // Clock it a few times
    let mut last = 0x100;
    for _ in 0..100 {
        if let Some(p) = unit.clock() {
            last = p;
        }
    }
    assert!(last < 0x100, "sweep should decrease period (negate)");
}

#[test]
fn test_sweep_mutes_when_period_too_low() {
    let mut unit = NesSweepUnit::new();
    unit.write_register(0x8F); // aggressive sweep
    unit.set_target_period(0x10);
    let mut muted = false;
    for _ in 0..100 {
        if unit.clock().is_none() {
            muted = true;
            break;
        }
    }
    assert!(muted, "should mute when period < 8");
}
```

### Task 7: Run tests, commit
