#![cfg(feature = "lang")]
use rill_core::traits::algorithm::Algorithm;
use rill_core::traits::{ParamValue, ProcessResult};
use rill_lang::builtin::{BlockBuiltin, BuiltinKind, BuiltinSig, Registry};

fn pv_f32(v: &ParamValue) -> f32 {
    match v {
        ParamValue::Float(f) => *f,
        ParamValue::Int(i) => *i as f32,
        _ => 0.0,
    }
}

fn pv_bool(v: &ParamValue) -> bool {
    match v {
        ParamValue::Bool(b) => *b,
        ParamValue::Float(f) => *f > 0.5,
        ParamValue::Int(i) => *i != 0,
        _ => false,
    }
}

struct LofiProcessor {
    config: crate::LofiConfig,
    last_sample: f32,
    sample_hold_counter: usize,
    reduction_factor: usize,
    delay_buffer: Vec<f32>,
    delay_write_pos: usize,
}

impl LofiProcessor {
    fn new(config: crate::LofiConfig) -> Self {
        let buf_sz = match config.system {
            crate::ClassicSystem::Nes => 256,
            crate::ClassicSystem::Commodore64 => 512,
            crate::ClassicSystem::AkaiS900 => 4096,
            crate::ClassicSystem::FairlightCMI => 2048,
            _ => 1024,
        };
        Self {
            config,
            last_sample: 0.0,
            sample_hold_counter: 0,
            reduction_factor: 1,
            delay_buffer: vec![0.0; buf_sz],
            delay_write_pos: 0,
        }
    }

    fn process_sample(&mut self, input: f32) -> f32 {
        let mut sample = input;

        if self.config.enable_sr_reduction {
            let target_sr = self.config.system.get_sample_rate();
            if target_sr > 0.0 {
                self.reduction_factor =
                    crate::dsp::quantization::calculate_reduction_factor(44100.0, target_sr);
            }
        }

        if self.config.enable_bitcrush {
            sample = crate::dsp::quantization::bitcrush(
                sample,
                self.config.system.get_bit_depth(),
                true,
            );
        }

        if self.config.enable_sr_reduction && self.reduction_factor > 1 {
            sample = crate::dsp::quantization::sample_rate_reduce(
                sample,
                self.reduction_factor,
                &mut self.last_sample,
                &mut self.sample_hold_counter,
            );
        }

        if self.config.enable_noise {
            sample = crate::dsp::noise::system_noise(self.config.system, sample);
        }

        sample = crate::dsp::dac_emulation::for_system(self.config.system, sample);

        if !self.delay_buffer.is_empty() {
            self.delay_buffer[self.delay_write_pos] = sample;
            let read_pos = if self.delay_write_pos >= 256 {
                self.delay_write_pos - 256
            } else {
                self.delay_buffer.len() + self.delay_write_pos - 256
            };
            let delayed = self.delay_buffer[read_pos];
            sample = sample * 0.7 + delayed * 0.3;
            self.delay_write_pos = (self.delay_write_pos + 1) % self.delay_buffer.len();
        }

        let wet = sample * self.config.dry_wet;
        let dry = input * (1.0 - self.config.dry_wet);
        let mut out = wet + dry;
        out -= self.config.dc_offset;
        out *= self.config.output_gain;
        out = out.clamp(-self.config.output_ceiling, self.config.output_ceiling);
        out
    }
}

impl Algorithm<f32> for LofiProcessor {
    fn process(&mut self, input: Option<&[f32]>, output: &mut [f32]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                for (i, out) in output.iter_mut().enumerate() {
                    *out = self.process_sample(inp[i.min(inp.len() - 1)]);
                }
            }
            None => output.fill(0.0),
        }
        Ok(())
    }

    fn reset(&mut self) {
        self.delay_buffer.fill(0.0);
        self.delay_write_pos = 0;
    }
}

impl LofiProcessor {
    fn update_param(&mut self, id: &rill_core::ParameterId, value: ParamValue) -> Result<(), ()> {
        match id.as_str() {
            "bit_depth" => {
                if let ParamValue::Int(v) = value {
                    self.config.hardware.bit_depth = v.clamp(1, 32) as u8;
                }
            }
            "sample_rate" => {
                if let ParamValue::Float(v) = value {
                    self.config.hardware.sample_rate = v;
                }
            }
            "dry_wet" => {
                if let ParamValue::Float(v) = value {
                    self.config.dry_wet = v.clamp(0.0, 1.0);
                }
            }
            "output_gain" => {
                if let ParamValue::Float(v) = value {
                    self.config.output_gain = v.max(0.0);
                }
            }
            "enable_bitcrush" => {
                if let ParamValue::Bool(v) = value {
                    self.config.enable_bitcrush = v;
                }
            }
            "enable_sr_reduction" => {
                if let ParamValue::Bool(v) = value {
                    self.config.enable_sr_reduction = v;
                }
            }
            "enable_noise" => {
                if let ParamValue::Bool(v) = value {
                    self.config.enable_noise = v;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl BlockBuiltin<f32> for LofiProcessor {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        use rill_core::ParameterId;
        let (name, pv) = match index {
            0 => ("bit_depth", ParamValue::Int(pv_f32(value).round() as i32)),
            1 => (
                "sample_rate",
                ParamValue::Float(pv_f32(value).clamp(8000.0, 192000.0)),
            ),
            2 => ("dry_wet", ParamValue::Float(pv_f32(value).clamp(0.0, 1.0))),
            3 => ("output_gain", ParamValue::Float(pv_f32(value).max(0.0))),
            4 => ("enable_bitcrush", ParamValue::Bool(pv_bool(value))),
            5 => ("enable_sr_reduction", ParamValue::Bool(pv_bool(value))),
            6 => ("enable_noise", ParamValue::Bool(pv_bool(value))),
            _ => return,
        };
        let _ = self.update_param(&ParameterId::new(name).unwrap(), pv);
    }
}

pub fn register_lofi_builtins(reg: &mut Registry<f32>) {
    use crate::ClassicSystem;

    reg.register_block(
        BuiltinSig::simple("lofi", 1, 1, 7, BuiltinKind::Block),
        |p, sr| {
            let config = crate::LofiConfig {
                system: ClassicSystem::Custom {
                    bit_depth: p[0].round() as u8,
                    sample_rate: p[1].clamp(8000.0, 192000.0) as f32,
                    nonlinear: false,
                    noise_floor: -48.0,
                },
                hardware: crate::HardwareEmulation {
                    bit_depth: p[0].round() as u8,
                    sample_rate: p[1].clamp(8000.0, 192000.0) as f32,
                    ..crate::HardwareEmulation::default()
                },
                enable_bitcrush: p[4] > 0.5,
                enable_sr_reduction: p[5] > 0.5,
                enable_noise: p[6] > 0.5,
                output_gain: p[3].max(0.0) as f32,
                dc_offset: 0.0,
                output_ceiling: 1.0,
                dry_wet: p[2].clamp(0.0, 1.0) as f32,
                ..Default::default()
            };
            let mut inner = LofiProcessor::new(config);
            Algorithm::init(&mut inner, sr);
            Box::new(LofiProcessor { ..inner })
        },
    );
}

struct Ay38910Builtin {
    chip: crate::Ay38910Chip,
}

impl Algorithm<f32> for Ay38910Builtin {
    fn process(&mut self, _input: Option<&[f32]>, output: &mut [f32]) -> ProcessResult<()> {
        self.chip.process(None, output)
    }
    fn init(&mut self, sr: f32) {
        Algorithm::init(&mut self.chip, sr);
    }
    fn reset(&mut self) {
        Algorithm::reset(&mut self.chip);
    }
}

impl BlockBuiltin<f32> for Ay38910Builtin {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        if index == 1 {
            if let ParamValue::Bytes(regs) = value {
                use crate::ChipEmulator;
                self.chip.write_registers(regs);
            }
        }
    }
}

pub fn register_chip_builtins(reg: &mut Registry<f32>) {
    use rill_core::traits::Algorithm;
    reg.register_block(
        BuiltinSig::simple("ay38910", 0, 1, 2, BuiltinKind::Block),
        |p, sr| {
            let clock = p[0] as f32;
            let mut chip = crate::Ay38910Chip::new(clock);
            Algorithm::init(&mut chip, sr);
            Box::new(Ay38910Builtin { chip })
        },
    );
}
