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

struct LofiBuiltin {
    inner: crate::LofiProcessor<64>,
}

impl Algorithm<f32> for LofiBuiltin {
    fn process(&mut self, input: Option<&[f32]>, output: &mut [f32]) -> ProcessResult<()> {
        match input {
            Some(inp) => {
                for (i, out) in output.iter_mut().enumerate() {
                    *out = self.inner.process_sample(inp[i.min(inp.len() - 1)]);
                }
            }
            None => output.fill(0.0),
        }
        Ok(())
    }
    fn reset(&mut self) {}
}

impl BlockBuiltin<f32> for LofiBuiltin {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        use rill_core::traits::Node;
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
        let _ = self
            .inner
            .set_parameter(&ParameterId::new(name).unwrap(), pv);
    }
}

pub fn register_lofi_builtins(reg: &mut Registry<f32>) {
    use crate::ClassicSystem;
    use rill_core::traits::Node;
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
                hardware: crate::HardwareEmulation::default(),
                enable_bitcrush: p[4] > 0.5,
                enable_sr_reduction: p[5] > 0.5,
                enable_noise: p[6] > 0.5,
                output_gain: p[3].max(0.0) as f32,
                dc_offset: 0.0,
                output_ceiling: 1.0,
                dry_wet: p[2].clamp(0.0, 1.0) as f32,
            };
            let mut inner = crate::LofiProcessor::<64>::new(config);
            Node::init(&mut inner, sr);
            Box::new(LofiBuiltin { inner })
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
