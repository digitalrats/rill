#[derive(Clone)]
struct NesPulseChannel {
    duty_cycle: f32,
    frequency: f32,
    volume: f32,
    phase: f32,
    sweep_enabled: bool,
    sweep_rate: f32,
    enabled: bool,
}

#[derive(Clone)]
struct NesTriangleChannel {
    frequency: f32,
    volume: f32,
    phase: f32,
    linear_counter: u8,
    enabled: bool,
}

struct NesNoiseChannel {
    mode: NoiseMode,
    frequency: f32,
    volume: f32,
    shift_register: u16,
    tick_counter: f32,
    enabled: bool,
}

#[derive(Clone)]
struct NesDpcmChannel {
    sample_rate: f32,
    delta: f32,
    sample_buffer: Vec<i8>,
    position: usize,
    current_output: f32,
    tick_counter: f32,
    enabled: bool,
}

#[derive(Debug, Clone, Copy)]
enum NoiseMode {
    Short,
    Long,
}

/// Pure NES 2A03 APU chip emulation logic.
///
/// No graph node, no lofi processing. Directly testable.
/// Registers are memory-mapped at $4000–$4015 (22 bytes).
pub struct NesChip {
    pulse1: NesPulseChannel,
    pulse2: NesPulseChannel,
    triangle: NesTriangleChannel,
    noise: NesNoiseChannel,
    dpcm: NesDpcmChannel,
}

impl NesChip {
    /// Create with default power-on state.
    pub fn new() -> Self {
        Self {
            pulse1: NesPulseChannel {
                duty_cycle: 0.25,
                frequency: 440.0,
                volume: 0.5,
                phase: 0.0,
                sweep_enabled: false,
                sweep_rate: 0.0,
                enabled: true,
            },
            pulse2: NesPulseChannel {
                duty_cycle: 0.125,
                frequency: 660.0,
                volume: 0.3,
                phase: 0.0,
                sweep_enabled: false,
                sweep_rate: 0.0,
                enabled: true,
            },
            triangle: NesTriangleChannel {
                frequency: 220.0,
                volume: 0.4,
                phase: 0.0,
                linear_counter: 0,
                enabled: true,
            },
            noise: NesNoiseChannel {
                mode: NoiseMode::Short,
                frequency: 1000.0,
                volume: 0.2,
                shift_register: 1,
                tick_counter: 0.0,
                enabled: true,
            },
            dpcm: NesDpcmChannel {
                sample_rate: 22050.0,
                delta: 0.01,
                sample_buffer: Vec::new(),
                position: 0,
                current_output: 0.0,
                tick_counter: 0.0,
                enabled: false,
            },
        }
    }

    /// Write register data. `regs` must be 22 bytes ($4000–$4015).
    pub fn write_registers(&mut self, regs: &[u8]) {
        if regs.len() < 22 {
            return;
        }

        let duty_table: [f32; 4] = [0.125, 0.25, 0.5, 0.75];

        // Pulse 1 ($4000–$4003)
        self.pulse1.duty_cycle = duty_table[((regs[0] >> 6) & 0x03) as usize];
        self.pulse1.volume = (regs[0] & 0x0F) as f32 / 15.0;
        let p1_period = (regs[2] as u16) | (((regs[3] as u16) & 0x07) << 8);
        self.pulse1.frequency = if p1_period > 0 {
            1_789_773.0 / (16.0 * (p1_period + 1) as f32)
        } else {
            0.0
        };

        // Pulse 2 ($4004–$4007)
        self.pulse2.duty_cycle = duty_table[((regs[4] >> 6) & 0x03) as usize];
        self.pulse2.volume = (regs[4] & 0x0F) as f32 / 15.0;
        let p2_period = (regs[6] as u16) | (((regs[7] as u16) & 0x07) << 8);
        self.pulse2.frequency = if p2_period > 0 {
            1_789_773.0 / (16.0 * (p2_period + 1) as f32)
        } else {
            0.0
        };

        // Triangle ($4008–$400B)
        self.triangle.volume = if (regs[8] & 0x80) != 0 { 0.4 } else { 0.0 };
        self.triangle.linear_counter = regs[8] & 0x7F;
        let tri_period = (regs[10] as u16) | (((regs[11] as u16) & 0x07) << 8);
        self.triangle.frequency = if tri_period > 0 {
            1_789_773.0 / (32.0 * (tri_period + 1) as f32)
        } else {
            0.0
        };

        // Noise ($400C–$400F)
        self.noise.mode = if (regs[12] & 0x80) != 0 {
            NoiseMode::Short
        } else {
            NoiseMode::Long
        };
        self.noise.volume = (regs[12] & 0x0F) as f32 / 15.0;
        let noise_period_idx = regs[14] & 0x0F;
        let noise_periods: [u16; 16] = [
            4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
        ];
        self.noise.frequency = 1_789_773.0 / noise_periods[noise_period_idx as usize] as f32;

        // Channel enable ($4015)
        let enable = regs[21];
        self.pulse1.enabled = (enable & 0x01) != 0;
        self.pulse2.enabled = (enable & 0x02) != 0;
        self.triangle.enabled = (enable & 0x04) != 0;
        self.noise.enabled = (enable & 0x08) != 0;
        self.dpcm.enabled = (enable & 0x10) != 0;
    }

    /// Generate one audio sample. `sample_rate` is the output sample rate.
    pub fn generate_sample(&mut self, sample_rate: f32) -> f32 {
        // Pulse 1
        let p1 = if self.pulse1.frequency > 0.0 && self.pulse1.enabled {
            self.pulse1.phase += self.pulse1.frequency / sample_rate;
            if self.pulse1.phase >= 1.0 {
                self.pulse1.phase -= 1.0;
            }
            (if self.pulse1.phase < self.pulse1.duty_cycle {
                1.0
            } else {
                -1.0
            }) * self.pulse1.volume
        } else {
            0.0
        };

        // Pulse 2
        let p2 = if self.pulse2.frequency > 0.0 && self.pulse2.enabled {
            self.pulse2.phase += self.pulse2.frequency / sample_rate;
            if self.pulse2.phase >= 1.0 {
                self.pulse2.phase -= 1.0;
            }
            (if self.pulse2.phase < self.pulse2.duty_cycle {
                1.0
            } else {
                -1.0
            }) * self.pulse2.volume
        } else {
            0.0
        };

        // Triangle
        let tri = if self.triangle.frequency > 0.0 && self.triangle.enabled {
            self.triangle.phase += self.triangle.frequency / sample_rate;
            if self.triangle.phase >= 1.0 {
                self.triangle.phase -= 1.0;
            }
            (if self.triangle.phase < 0.5 {
                self.triangle.phase * 4.0 - 1.0
            } else {
                3.0 - self.triangle.phase * 4.0
            }) * self.triangle.volume
        } else {
            0.0
        };

        // Noise
        let ns = if self.noise.enabled {
            self.generate_noise(sample_rate)
        } else {
            0.0
        };

        // DPCM
        let dpcm = if self.dpcm.enabled {
            self.generate_dpcm(sample_rate)
        } else {
            0.0
        };

        let pulse_mix = (p1 + p2) * 0.5;
        let tnd_mix = (tri * 3.0 + ns * 2.0 + dpcm) / 6.0;
        (pulse_mix * 0.5 + tnd_mix * 0.5) * 0.5
    }

    pub fn reset(&mut self) {
        self.pulse1.phase = 0.0;
        self.pulse2.phase = 0.0;
        self.triangle.phase = 0.0;
        self.noise.shift_register = 1;
        self.noise.tick_counter = 0.0;
        self.dpcm.position = 0;
        self.dpcm.current_output = 0.0;
        self.dpcm.tick_counter = 0.0;
    }

    fn generate_noise(&mut self, sample_rate: f32) -> f32 {
        let ticks_per_sample = sample_rate / self.noise.frequency;
        self.noise.tick_counter += 1.0;
        if self.noise.tick_counter >= ticks_per_sample {
            self.noise.tick_counter = 0.0;
            let feedback = match self.noise.mode {
                NoiseMode::Short => {
                    (self.noise.shift_register & 0x0001)
                        ^ ((self.noise.shift_register >> 6) & 0x0001)
                }
                NoiseMode::Long => {
                    (self.noise.shift_register & 0x0001)
                        ^ ((self.noise.shift_register >> 1) & 0x0001)
                }
            };
            self.noise.shift_register >>= 1;
            self.noise.shift_register |= feedback << 14;
        }
        let sample = if (self.noise.shift_register & 0x0001) == 0 {
            1.0
        } else {
            -1.0
        };
        sample * self.noise.volume
    }

    fn generate_dpcm(&mut self, sample_rate: f32) -> f32 {
        if self.dpcm.sample_buffer.is_empty()
            || self.dpcm.position >= self.dpcm.sample_buffer.len() * 8
        {
            return self.dpcm.current_output;
        }
        let ticks_per_sample = sample_rate / self.dpcm.sample_rate;
        self.dpcm.tick_counter += 1.0;
        if self.dpcm.tick_counter >= ticks_per_sample {
            self.dpcm.tick_counter = 0.0;
            let byte_idx = self.dpcm.position / 8;
            let bit_idx = self.dpcm.position % 8;
            if byte_idx < self.dpcm.sample_buffer.len() {
                let bit = (self.dpcm.sample_buffer[byte_idx] >> bit_idx) & 1;
                if bit != 0 {
                    self.dpcm.current_output =
                        (self.dpcm.current_output + self.dpcm.delta).min(1.0);
                } else {
                    self.dpcm.current_output =
                        (self.dpcm.current_output - self.dpcm.delta).max(-1.0);
                }
                self.dpcm.position += 1;
            }
        }
        self.dpcm.current_output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nes_chip_silent_when_all_disabled() {
        let mut chip = NesChip::new();
        let mut regs = [0u8; 22];
        regs[21] = 0x00; // all channels disabled
        chip.write_registers(&regs);
        let s = chip.generate_sample(44100.0);
        assert!(s.abs() < 0.001, "all disabled should be silent, got {}", s);
    }

    #[test]
    fn test_nes_chip_produces_audio() {
        let mut chip = NesChip::new();
        let mut regs = [0u8; 22];
        // Pulse 1: 50% duty, max volume, period=0x100 (~438 Hz)
        regs[0] = 0x8F; // duty=10 (50%), volume=15
        regs[2] = 0x00; // period low
        regs[3] = 0x01; // period high
        regs[21] = 0x01; // pulse1 enabled
        chip.write_registers(&regs);
        let mut max_abs = 0.0f32;
        for _ in 0..1024 {
            let s = chip.generate_sample(44100.0);
            max_abs = max_abs.max(s.abs());
        }
        assert!(max_abs > 0.1, "should produce audio, max_abs={}", max_abs);
    }

    #[test]
    fn test_nes_chip_reset() {
        let mut chip = NesChip::new();
        let mut regs = [0u8; 22];
        regs[21] = 0x01; // pulse1 enabled
        chip.write_registers(&regs);
        for _ in 0..100 {
            chip.generate_sample(44100.0);
        }
        chip.reset();
        assert_eq!(chip.pulse1.phase, 0.0);
        assert_eq!(chip.noise.shift_register, 1);
    }
}
