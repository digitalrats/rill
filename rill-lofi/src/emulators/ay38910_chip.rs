use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::parameter_write::ParameterWrite;
use rill_core::traits::{ParamValue, ProcessError, ProcessResult};

use crate::chip_emulator::ChipEmulator;

#[derive(Clone)]
struct AyChannel {
    tone_period: u16,
    volume: u8,
    phase: f32,
    use_envelope: bool,
}

#[derive(Clone)]
struct AyNoise {
    period: u8,
    shift_register: u32,
    output: bool,
    phase: f32,
}

#[derive(Clone)]
struct AyEnvelope {
    period: u16,
    mode: u8,
    phase: f32,
    value: u8,
    counter: u32,
}

#[derive(Clone)]
struct AyMixer {
    channel_modes: [u8; 3],
    io_a_enabled: bool,
    io_b_enabled: bool,
}

/// Pure AY-3-8910 / YM2149 chip emulation logic.
///
/// No graph node, no lofi processing, no I/O backend.
/// Directly testable. `sample_rate` is passed to `generate_sample`.
pub struct Ay38910Chip {
    channels: [AyChannel; 3],
    noise: AyNoise,
    envelope: AyEnvelope,
    mixer: AyMixer,
    pub(crate) chip_clock: f32,
    pub(crate) registers: [u8; 16],
    pub(crate) registers_dirty: bool,
    sample_rate: f32,
}

impl Ay38910Chip {
    /// Create a new AY-3-8910 chip with the given master clock frequency.
    pub fn new(chip_clock: f32) -> Self {
        Self {
            channels: [
                AyChannel {
                    tone_period: 0,
                    volume: 0,
                    phase: 0.0,
                    use_envelope: false,
                },
                AyChannel {
                    tone_period: 0,
                    volume: 0,
                    phase: 0.0,
                    use_envelope: false,
                },
                AyChannel {
                    tone_period: 0,
                    volume: 0,
                    phase: 0.0,
                    use_envelope: false,
                },
            ],
            noise: AyNoise {
                period: 0,
                shift_register: 0x0001_0000,
                output: false,
                phase: 0.0,
            },
            envelope: AyEnvelope {
                period: 0,
                mode: 0,
                phase: 0.0,
                value: 0,
                counter: 0,
            },
            mixer: AyMixer {
                channel_modes: [0, 0, 0],
                io_a_enabled: false,
                io_b_enabled: false,
            },
            chip_clock,
            registers: [0; 16],
            registers_dirty: true,
            sample_rate: 44100.0,
        }
    }

    /// Write a value to one of the 16 chip registers.
    pub fn write_register(&mut self, reg: usize, value: u8) {
        if reg < 16 {
            self.registers[reg] = value;
            self.registers_dirty = true;
        }
    }

    /// Read the current value of a chip register. Returns 0 for out-of-range.
    pub fn read_register(&self, reg: usize) -> u8 {
        if reg < 16 {
            self.registers[reg]
        } else {
            0
        }
    }

    /// Generate one audio sample at the given output sample rate.
    pub fn generate_sample(&mut self, sample_rate: f32) -> f32 {
        if self.registers_dirty {
            self.update_from_registers();
            self.registers_dirty = false;
        }
        let chip_clock = self.chip_clock;
        let mut channel_samples = [0.0f32; 3];
        for (i, channel) in self.channels.iter_mut().enumerate() {
            if channel.tone_period > 0 {
                let tone_freq = chip_clock / (16.0 * channel.tone_period as f32);
                let phase_inc = tone_freq / sample_rate;
                channel.phase += phase_inc;
                while channel.phase >= 1.0 {
                    channel.phase -= 1.0;
                }
            }
            let tone_enabled = (self.mixer.channel_modes[i] & 0x01) == 0;
            let noise_enabled = (self.mixer.channel_modes[i] & 0x02) == 0;
            let tone_bit = 1.0; // DEBUG: tones disabled, noise only
            let noise_bit = if noise_enabled {
                if self.noise.output {
                    1.0
                } else {
                    0.0
                }
            } else {
                1.0
            };
            let digital_out = tone_bit * noise_bit;
            let volume = if channel.use_envelope {
                self.envelope.value as f32 / 15.0
            } else {
                channel.volume as f32 / 15.0
            };
            channel_samples[i] = digital_out * volume;
        }
        self.update_noise(sample_rate);
        self.update_envelope(sample_rate);
        (channel_samples[0] + channel_samples[1] + channel_samples[2]) / 3.0
    }

    /// Reset chip registers and internal state to power-on defaults.
    pub fn reset(&mut self) {
        self.registers = [0; 16];
        self.registers_dirty = true;
        for ch in &mut self.channels {
            ch.phase = 0.0;
        }
        self.noise.shift_register = 0x0001_0000;
        self.noise.output = false;
        self.noise.phase = 0.0;
        self.envelope.phase = 0.0;
        self.envelope.value = 0;
        self.envelope.counter = 0;
    }

    pub(crate) fn update_from_registers(&mut self) {
        self.channels[0].tone_period =
            ((self.registers[1] as u16 & 0x0F) << 8) | (self.registers[0] as u16);
        self.channels[1].tone_period =
            ((self.registers[3] as u16 & 0x0F) << 8) | (self.registers[2] as u16);
        self.channels[2].tone_period =
            ((self.registers[5] as u16 & 0x0F) << 8) | (self.registers[4] as u16);
        self.noise.period = self.registers[6] & 0x1F;
        let mixer_reg = self.registers[7];
        self.mixer.channel_modes[0] = ((mixer_reg >> 3) & 0x01) << 1 | (mixer_reg & 0x01);
        self.mixer.channel_modes[1] = ((mixer_reg >> 4) & 0x01) << 1 | ((mixer_reg >> 1) & 0x01);
        self.mixer.channel_modes[2] = ((mixer_reg >> 5) & 0x01) << 1 | ((mixer_reg >> 2) & 0x01);
        self.mixer.io_a_enabled = (mixer_reg & 0x40) == 0;
        self.mixer.io_b_enabled = (mixer_reg & 0x80) == 0;
        for i in 0..3 {
            let vol_reg = self.registers[8 + i];
            self.channels[i].use_envelope = (vol_reg & 0x10) != 0;
            self.channels[i].volume = vol_reg & 0x0F;
        }
        self.envelope.period = ((self.registers[12] as u16) << 8) | (self.registers[11] as u16);
        self.envelope.mode = self.registers[13] & 0x0F;
    }

    fn update_noise(&mut self, sample_rate: f32) {
        if self.noise.period == 0 {
            return;
        }
        let noise_freq = self.chip_clock / (16.0 * self.noise.period as f32);
        let inc = noise_freq / sample_rate;
        self.noise.phase += inc;
        while self.noise.phase >= 1.0 {
            self.noise.phase -= 1.0;
            let output_bit = (self.noise.shift_register & 1) != 0;
            let feedback =
                ((self.noise.shift_register >> 16) ^ (self.noise.shift_register >> 13)) & 1;
            self.noise.shift_register = ((self.noise.shift_register << 1) | feedback) & 0x1FFFF;
            self.noise.output = output_bit;
        }
    }

    fn update_envelope(&mut self, sample_rate: f32) {
        if self.envelope.period == 0 {
            self.envelope.value = 0;
            return;
        }
        let env_freq = self.chip_clock / (256.0 * self.envelope.period as f32);
        let inc = env_freq / sample_rate;
        self.envelope.phase += inc;
        while self.envelope.phase >= 1.0 {
            self.envelope.phase -= 1.0;
            self.handle_envelope_tick();
        }
    }

    fn handle_envelope_tick(&mut self) {
        let mode = self.envelope.mode;
        let cont = (mode & 0x08) != 0;
        let attack = (mode & 0x04) != 0;
        let alt = (mode & 0x02) != 0;
        let hold = (mode & 0x01) != 0;
        let step = self.envelope.counter;
        let half_cycle = step / 16;
        let sub_step = step % 16;
        let ramp_up = attack ^ (alt && (half_cycle & 1) == 1);
        let done = !cont && if alt { step >= 32 } else { step >= 16 };
        if done {
            if !hold {
                self.envelope.value = 0;
            }
        } else {
            self.envelope.value = if ramp_up {
                sub_step as u8
            } else {
                15u8.saturating_sub(sub_step as u8)
            };
        }
        self.envelope.counter += 1;
    }
}

impl Algorithm<f32> for Ay38910Chip {
    fn process(&mut self, _input: Option<&[f32]>, output: &mut [f32]) -> ProcessResult<()> {
        if self.registers_dirty {
            self.update_from_registers();
            self.registers_dirty = false;
        }
        for s in output.iter_mut() {
            *s = self.generate_sample(self.sample_rate);
        }
        Ok(())
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }

    fn reset(&mut self) {
        self.registers = [0; 16];
        self.registers_dirty = true;
        for ch in &mut self.channels {
            ch.phase = 0.0;
            ch.tone_period = 0;
            ch.volume = 0;
            ch.use_envelope = false;
        }
        self.noise = AyNoise {
            period: 0,
            shift_register: 0x0001_0000,
            output: false,
            phase: 0.0,
        };
        self.envelope = AyEnvelope {
            period: 0,
            mode: 0,
            phase: 0.0,
            value: 0,
            counter: 0,
        };
        self.mixer.channel_modes = [0; 3];
        self.mixer.io_a_enabled = false;
        self.mixer.io_b_enabled = false;
    }
}

impl ChipEmulator for Ay38910Chip {
    fn write_registers(&mut self, regs: &[u8]) {
        if regs.len() >= 14 {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let hex: Vec<String> = regs[..14].iter().map(|b| format!("{b:02x}")).collect();
            eprintln!("[AY] ts={t} regs=[{}]", hex.join(" "));
        }
        for (i, &v) in regs.iter().enumerate().take(16) {
            self.write_register(i, v);
        }
    }
}

impl ParameterWrite for Ay38910Chip {
    fn write_parameter(&mut self, name: &str, value: ParamValue) -> ProcessResult<()> {
        match name {
            "register_write" => {
                if let Some(bytes) = value.as_bytes() {
                    self.write_registers(bytes);
                    return Ok(());
                }
                Err(ProcessError::parameter("register_write expects Bytes"))
            }
            _ => Err(ProcessError::parameter(format!(
                "unknown parameter: {name}"
            ))),
        }
    }
}

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
    fn test_mixer_register_bit_mapping() {
        let mut chip = Ay38910Chip::new(1_750_000.0);
        chip.write_register(7, 0b11_10_01_00);
        chip.generate_sample(SR);
        assert_eq!(chip.mixer.channel_modes[0], 0b00, "Ch A mode");
        assert_eq!(chip.mixer.channel_modes[1], 0b00, "Ch B mode");
        assert_eq!(chip.mixer.channel_modes[2], 0b11, "Ch C mode");
    }

    #[test]
    fn test_noise_disabled_is_silent() {
        let mut chip = Ay38910Chip::new(1_750_000.0);
        chip.write_register(7, 0xFF);
        let s = chip.generate_sample(SR);
        assert!(s.abs() < 0.001, "muted chip should be silent, got {}", s);
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

    #[test]
    fn test_noise_lfsr_produces_sequence() {
        let mut chip = Ay38910Chip::new(1_750_000.0);
        chip.write_register(6, 4);
        chip.write_register(10, 15);
        chip.write_register(7, 0b11_11_00_00);
        let mut last = chip.noise.output;
        let mut toggles = 0usize;
        for _ in 0..4096 {
            chip.generate_sample(SR);
            let current = chip.noise.output;
            if current != last {
                toggles += 1;
                last = current;
            }
        }
        assert!(
            toggles > 100,
            "noise LFSR should have many transitions, got {}",
            toggles
        );
    }

    #[test]
    fn test_tone_frequency_accuracy() {
        let mut chip = Ay38910Chip::new(1_750_000.0);
        let divider = 248u16;
        let expected_freq = 1_750_000.0 / (16.0 * divider as f32);
        chip.write_register(0, divider as u8);
        chip.write_register(1, (divider >> 8) as u8);
        chip.write_register(8, 15);
        chip.write_register(7, 0b11_11_11_10);
        chip.generate_sample(SR);
        let n_samples = 44100;
        let mut crossings = 0usize;
        let mut prev_phase = chip.channels[0].phase;
        for _ in 0..n_samples {
            chip.generate_sample(SR);
            let phase = chip.channels[0].phase;
            if prev_phase > 0.9 && phase < 0.1 {
                crossings += 1;
            }
            prev_phase = phase;
        }
        let measured_hz = crossings as f32 * SR / n_samples as f32;
        let diff = (measured_hz - expected_freq).abs() / expected_freq;
        assert!(
            diff < 0.05,
            "tone freq: expected ~{:.1}Hz, measured {:.1}Hz",
            expected_freq,
            measured_hz
        );
    }
}
