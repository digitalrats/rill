use kama_core::{AudioNode, ParamValue, NodeMetadata, NodeCategory, AudioError, node::ParamMetadata};
use kama_core::param::ParamType;
use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;

// AY-3-8910 specific structures
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
    noise_freq: f32,
    output: bool,
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

/// Эмулятор AY-3-8910 / YM2149 (ZX Spectrum 128, Atari ST, Amstrad CPC)
pub struct Ay38910Emulator {
    channels: [AyChannel; 3],
    noise: AyNoise,
    envelope: AyEnvelope,
    mixer: AyMixer,
    sample_rate: f32,
    chip_clock: f32,
    registers: [u8; 16],
    registers_dirty: bool,
    lofi: LofiProcessor,
}

impl Ay38910Emulator {
    pub fn new(sample_rate: f32) -> Self {
        let chip_clock = 1_750_000.0;
        
        let lofi_config = LofiConfig::for_system(crate::config::ClassicSystem::Custom {
            bit_depth: 8,
            sample_rate: 44100.0,
            nonlinear: false,
            noise_floor: -48.0,
        });
        
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
                noise_freq: 0.0,
                output: false,
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
            sample_rate,
            chip_clock,
            registers: [0; 16],
            registers_dirty: true,
            lofi: LofiProcessor::new(lofi_config),
        }
    }
    
    pub fn write_register(&mut self, reg: usize, value: u8) {
        if reg < 16 {
            self.registers[reg] = value;
            self.registers_dirty = true;
            self.update_from_registers();
        }
    }
    
    pub fn read_register(&self, reg: usize) -> u8 {
        if reg < 16 {
            self.registers[reg]
        } else {
            0
        }
    }
    
    fn update_from_registers(&mut self) {
        // Channel A
        self.channels[0].tone_period = ((self.registers[1] as u16 & 0x0F) << 8) | (self.registers[0] as u16);
        
        // Channel B
        self.channels[1].tone_period = ((self.registers[3] as u16 & 0x0F) << 8) | (self.registers[2] as u16);
        
        // Channel C
        self.channels[2].tone_period = ((self.registers[5] as u16 & 0x0F) << 8) | (self.registers[4] as u16);
        
        // Noise period
        self.noise.period = self.registers[6] & 0x1F;
        if self.noise.period > 0 {
            self.noise.noise_freq = self.chip_clock / (16.0 * self.noise.period as f32);
        } else {
            self.noise.noise_freq = 0.0;
        }
        
        // Mixer
        let mixer_reg = self.registers[7];
        self.mixer.channel_modes[0] = (mixer_reg & 0x03) as u8;
        self.mixer.channel_modes[1] = ((mixer_reg >> 2) & 0x03) as u8;
        self.mixer.channel_modes[2] = ((mixer_reg >> 4) & 0x03) as u8;
        self.mixer.io_a_enabled = (mixer_reg & 0x40) == 0;
        self.mixer.io_b_enabled = (mixer_reg & 0x80) == 0;
        
        // Channel volume
        for i in 0..3 {
            let vol_reg = self.registers[8 + i];
            self.channels[i].use_envelope = (vol_reg & 0x10) != 0;
            self.channels[i].volume = vol_reg & 0x0F;
        }
        
        // Envelope
        self.envelope.period = ((self.registers[12] as u16) << 8) | (self.registers[11] as u16);
        self.envelope.mode = self.registers[13] & 0x0F;
    }
    
    pub fn generate_sample(&mut self) -> f32 {
        if self.registers_dirty {
            self.update_from_registers();
            self.registers_dirty = false;
        }
        
        let sample_rate = self.sample_rate;
        let chip_clock = self.chip_clock;
        
        let mut channel_samples = [0.0f32; 3];
        
        for i in 0..3 {
            let channel = &mut self.channels[i];
            
            if channel.tone_period > 0 {
                let tone_freq = chip_clock / (16.0 * channel.tone_period as f32);
                let phase_inc = tone_freq / sample_rate;
                
                channel.phase += phase_inc;
                if channel.phase >= 1.0 {
                    channel.phase -= 1.0;
                }
            }
            
            let tone_enabled = (self.mixer.channel_modes[i] & 0x01) == 0;
            let noise_enabled = (self.mixer.channel_modes[i] & 0x02) == 0;
            
            let tone_sample = if tone_enabled && channel.tone_period > 0 {
                if channel.phase < 0.5 { 1.0 } else { -1.0 }
            } else {
                0.0
            };
            
            // ИСПРАВЛЕНО: правильный синтаксис для вложенного if
            let noise_sample = if noise_enabled {
                if self.noise.output { 1.0 } else { -1.0 }
            } else {
                0.0
            };
            
            let mixed = (tone_sample + noise_sample) * 0.5;
            
            let volume = if channel.use_envelope {
                self.envelope.value as f32 / 15.0
            } else {
                channel.volume as f32 / 15.0
            };
            
            channel_samples[i] = mixed * volume;
        }
        
        self.update_noise();
        self.update_envelope();
        
        let mixed = (channel_samples[0] + channel_samples[1] + channel_samples[2]) / 3.0;
        
        self.lofi.process_sample(mixed)
    }

    fn update_noise(&mut self) {
        if self.noise.period == 0 {
            return;
        }
        
        let noise_freq = self.noise.noise_freq;
        let increments_per_sample = noise_freq / self.sample_rate;
        
        static mut NOISE_PHASE: f32 = 0.0;
        unsafe {
            NOISE_PHASE += increments_per_sample;
            if NOISE_PHASE >= 1.0 {
                NOISE_PHASE -= 1.0;
                
                let feedback = (self.noise.shift_register >> 16) ^ 
                               (self.noise.shift_register >> 13) & 1;
                self.noise.shift_register = ((self.noise.shift_register << 1) | feedback) & 0x1FFFF;
                
                self.noise.output = (self.noise.shift_register >> 16) != 0;
            }
        }
    }
    
    fn update_envelope(&mut self) {
        if self.envelope.period == 0 {
            self.envelope.value = 0;
            return;
        }
        
        let env_freq = self.chip_clock / (16.0 * self.envelope.period as f32);
        let increments_per_sample = env_freq / self.sample_rate;
        
        self.envelope.phase += increments_per_sample;
        
        if self.envelope.phase >= 1.0 {
            self.envelope.phase -= 1.0;
            self.envelope.counter += 1;
            
            let cont = (self.envelope.mode & 0x08) != 0;
            let attack = (self.envelope.mode & 0x04) != 0;
            let hold = (self.envelope.mode & 0x02) != 0;
            let repeat = (self.envelope.mode & 0x01) != 0;
            
            let max_steps = 16u32;
            
            if !cont {
                if self.envelope.counter < max_steps {
                    self.envelope.value = if attack {
                        self.envelope.counter as u8
                    } else {
                        (max_steps - 1 - self.envelope.counter) as u8
                    };
                } else {
                    self.envelope.value = if hold {
                        if attack { 15 } else { 0 }
                    } else {
                        0
                    };
                }
            } else {
                let cycle_pos = self.envelope.counter % max_steps;
                
                if !hold && repeat {
                    if attack {
                        self.envelope.value = cycle_pos as u8;
                    } else {
                        self.envelope.value = (max_steps - 1 - cycle_pos) as u8;
                    }
                } else if hold && !repeat {
                    if self.envelope.counter < max_steps {
                        self.envelope.value = if attack {
                            cycle_pos as u8
                        } else {
                            (max_steps - 1 - cycle_pos) as u8
                        };
                    }
                } else {
                    self.envelope.value = if attack {
                        cycle_pos as u8
                    } else {
                        (max_steps - 1 - cycle_pos) as u8
                    };
                }
            }
        }
    }
    
    pub fn reset(&mut self) {
        self.registers = [0; 16];
        self.registers_dirty = true;
        
        for channel in &mut self.channels {
            channel.phase = 0.0;
        }
        
        self.noise.shift_register = 0x0001_0000;
        self.noise.output = false;
        
        self.envelope.phase = 0.0;
        self.envelope.value = 0;
        self.envelope.counter = 0;
        
        self.lofi.reset();
    }
}

impl AudioNode for Ay38910Emulator {
    fn process(&mut self, _inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if outputs.is_empty() {
            return Ok(());
        }
        
        let output = &mut outputs[0];
        
        for out in output.iter_mut() {
            *out = self.generate_sample();
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "chip_clock" => Some(ParamValue::Float(self.chip_clock)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("chip_clock", ParamValue::Float(v)) => {
                self.chip_clock = v.max(1_000_000.0).min(4_000_000.0);
                self.registers_dirty = true;
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.lofi.init(sample_rate);
    }
    
    fn reset(&mut self) {
        self.reset();
    }
    
    fn num_inputs(&self) -> usize { 0 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "AY-3-8910".to_string(),
            category: NodeCategory::Generator,
            description: "AY-3-8910 / YM2149 sound chip emulation (ZX Spectrum 128, Atari ST, Amstrad CPC)".to_string(),
            author: "Kama Lo-Fi".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "chip_clock".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1_750_000.0),
                    min: Some(1_000_000.0),
                    max: Some(4_000_000.0),
                    step: Some(100_000.0),
                    unit: Some("Hz".to_string()),
                    choices: Some(vec![
                        ("ZX Spectrum (1.75 MHz)".to_string(), 1_750_000.0),
                        ("Atari ST (2.0 MHz)".to_string(), 2_000_000.0),
                        ("Amstrad CPC (1.0 MHz)".to_string(), 1_000_000.0),
                    ]),
                },
            ],
        }
    }
}