use crate::config::{ClassicSystem, LofiConfig};
use crate::dsp;
use rill_core::prelude::*;

pub struct LofiProcessor<const BUF_SIZE: usize> {
    state: NodeState<f32, BUF_SIZE>,
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<f32, BUF_SIZE>>,
    outputs: Vec<Port<f32, BUF_SIZE>>,
    #[allow(dead_code)]
    controls: Vec<Port<f32, BUF_SIZE>>,

    config: LofiConfig,
    delay_buffer: Vec<f32>,
    delay_write_pos: usize,
    last_sample: f32,
    sample_hold_counter: usize,
    reduction_factor: usize,
}

impl<const BUF_SIZE: usize> LofiProcessor<BUF_SIZE> {
    pub fn new(config: LofiConfig) -> Self {
        let buffer_size = match config.system {
            ClassicSystem::Nes => 256,
            ClassicSystem::Commodore64 => 512,
            ClassicSystem::AkaiS900 => 4096,
            ClassicSystem::FairlightCMI => 2048,
            _ => 1024,
        };

        let metadata = Self::build_metadata(&config);
        let id = NodeId(0);
        let state = NodeState::new(44100.0);

        let inputs = vec![Port::input(id, 0, "audio_in")];
        let outputs = vec![Port::output(id, 0, "audio_out")];

        Self {
            state,
            id,
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            config,
            delay_buffer: vec![0.0; buffer_size],
            delay_write_pos: 0,
            last_sample: 0.0,
            sample_hold_counter: 0,
            reduction_factor: 1,
        }
    }

    pub fn for_system(system: ClassicSystem) -> Self {
        Self::new(LofiConfig::for_system(system))
    }

    pub fn process_sample(&mut self, input: f32) -> f32 {
        let mut sample = input;

        if self.config.enable_sr_reduction {
            let target_sr = self.config.system.get_sample_rate();
            self.reduction_factor =
                dsp::quantization::calculate_reduction_factor(self.state.sample_rate, target_sr);
        }

        if self.config.enable_bitcrush {
            let bit_depth = self.config.system.get_bit_depth();
            sample = dsp::quantization::bitcrush(sample, bit_depth, true);
        }

        if self.config.enable_sr_reduction && self.reduction_factor > 1 {
            sample = dsp::quantization::sample_rate_reduce(
                sample,
                self.reduction_factor,
                &mut self.last_sample,
                &mut self.sample_hold_counter,
            );
        }

        if self.config.enable_noise {
            sample = dsp::noise::system_noise(self.config.system, sample);
        }

        sample = dsp::dac_emulation::for_system(self.config.system, sample);

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

        (wet + dry) * self.config.output_gain
    }

    pub fn clear_delay_buffer(&mut self) {
        self.delay_buffer.fill(0.0);
        self.delay_write_pos = 0;
    }

    pub fn stats(&self) -> (u64, f32) {
        (
            self.state.sample_pos,
            self.state.current_time_seconds() as f32,
        )
    }

    fn build_metadata(config: &LofiConfig) -> NodeMetadata {
        let system_name = match config.system {
            ClassicSystem::Nes => "NES Emulator",
            ClassicSystem::Commodore64 => "Commodore 64 SID",
            ClassicSystem::AkaiS900 => "Akai S900 Sampler",
            ClassicSystem::FairlightCMI => "Fairlight CMI",
            ClassicSystem::Custom { .. } => "Custom Lo-Fi",
            _ => "Lo-Fi Processor",
        };

        let description = match config.system {
            ClassicSystem::Nes => "Nintendo Entertainment System sound chip".to_string(),
            ClassicSystem::Commodore64 => "Commodore 64 SID chip".to_string(),
            ClassicSystem::AkaiS900 => "Akai S900 12-bit sampler".to_string(),
            ClassicSystem::FairlightCMI => "Fairlight CMI (first digital sampler)".to_string(),
            ClassicSystem::Custom {
                bit_depth,
                sample_rate,
                ..
            } => format!("Custom {}-bit at {} Hz", bit_depth, sample_rate),
            _ => "vintage digital audio system".to_string(),
        };

        NodeMetadata {
            name: system_name.to_string(),
            
            type_name: None,category: NodeCategory::Processor,
            description,
            author: "Rill Lo-Fi".to_string(),
            version: "0.2.0".to_string(),
            audio_inputs: 1,
            audio_outputs: 1,
            control_inputs: 0,
            control_outputs: 0,
            clock_inputs: 0,
            clock_outputs: 0,
            feedback_ports: 0,
            parameters: vec![
                ParamMetadata::new(
                    "system",
                    ParamType::Choice,
                    ParamValue::Choice("NES".to_string()),
                )
                .with_description("Classic system to emulate")
                .with_choices(vec![
                    ("NES".to_string(), 0.0),
                    ("Commodore64".to_string(), 1.0),
                    ("AkaiS900".to_string(), 2.0),
                    ("FairlightCMI".to_string(), 3.0),
                    ("Custom".to_string(), 4.0),
                ]),
                ParamMetadata::new("bit_depth", ParamType::Int, ParamValue::Int(8))
                    .with_description("Bit depth for quantization")
                    .with_range(1.0, 16.0, 1.0)
                    .with_unit("bits"),
                ParamMetadata::new("dry_wet", ParamType::Float, ParamValue::Float(1.0))
                    .with_description("Dry/wet mix")
                    .with_range(0.0, 1.0, 0.01),
                ParamMetadata::new("output_gain", ParamType::Float, ParamValue::Float(1.0))
                    .with_description("Output gain")
                    .with_range(0.0, 4.0, 0.1),
                ParamMetadata::new("enable_bitcrush", ParamType::Bool, ParamValue::Bool(true))
                    .with_description("Enable bitcrushing"),
                ParamMetadata::new(
                    "enable_sr_reduction",
                    ParamType::Bool,
                    ParamValue::Bool(true),
                )
                .with_description("Enable sample rate reduction"),
                ParamMetadata::new("enable_noise", ParamType::Bool, ParamValue::Bool(true))
                    .with_description("Enable vintage noise"),
            ],
        }
    }
}

impl<const BUF_SIZE: usize> SignalNode<f32, BUF_SIZE> for LofiProcessor<BUF_SIZE> {
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }

    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }

    fn init(&mut self, sample_rate: f32) {
        self.state = NodeState::new(sample_rate);
        self.last_sample = 0.0;
        self.sample_hold_counter = 0;
        self.clear_delay_buffer();

        if let ClassicSystem::Custom {
            sample_rate: ref mut field_sr,
            ..
        } = self.config.system
        {
            *field_sr = sample_rate;
        }

        if self.config.enable_sr_reduction {
            let target_sr = self.config.system.get_sample_rate();
            self.reduction_factor =
                dsp::quantization::calculate_reduction_factor(sample_rate, target_sr);
        }
    }

    fn reset(&mut self) {
        self.state.reset();
        self.last_sample = 0.0;
        self.sample_hold_counter = 0;
        self.clear_delay_buffer();
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "bit_depth" => Some(ParamValue::Int(self.config.system.get_bit_depth() as i32)),
            "sample_rate" => Some(ParamValue::Float(self.config.system.get_sample_rate())),
            "dry_wet" => Some(ParamValue::Float(self.config.dry_wet)),
            "output_gain" => Some(ParamValue::Float(self.config.output_gain)),
            "enable_bitcrush" => Some(ParamValue::Bool(self.config.enable_bitcrush)),
            "enable_sr_reduction" => Some(ParamValue::Bool(self.config.enable_sr_reduction)),
            "enable_noise" => Some(ParamValue::Bool(self.config.enable_noise)),
            "system" => {
                let name = match self.config.system {
                    ClassicSystem::Nes => "NES",
                    ClassicSystem::Commodore64 => "Commodore64",
                    ClassicSystem::AkaiS900 => "AkaiS900",
                    ClassicSystem::FairlightCMI => "FairlightCMI",
                    ClassicSystem::Custom { .. } => "Custom",
                    _ => "Unknown",
                };
                Some(ParamValue::Choice(name.to_string()))
            }
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match id.as_str() {
            "bit_depth" => {
                if let ParamValue::Int(v) = value {
                    if let ClassicSystem::Custom {
                        ref mut bit_depth, ..
                    } = self.config.system
                    {
                        *bit_depth = v as u8;
                        return Ok(());
                    }
                }
                Err(ProcessError::parameter(
                    "Cannot change bit_depth of fixed system",
                ))
            }
            "sample_rate" => {
                if let ParamValue::Float(v) = value {
                    if let ClassicSystem::Custom {
                        ref mut sample_rate,
                        ..
                    } = self.config.system
                    {
                        *sample_rate = v.clamp(8000.0, 192000.0);
                        return Ok(());
                    }
                }
                Err(ProcessError::parameter(
                    "Cannot change sample_rate of fixed system",
                ))
            }
            "dry_wet" => {
                if let ParamValue::Float(v) = value {
                    self.config.dry_wet = v.clamp(0.0, 1.0);
                    return Ok(());
                }
                Err(ProcessError::parameter("dry_wet must be a float"))
            }
            "output_gain" => {
                if let ParamValue::Float(v) = value {
                    self.config.output_gain = v.clamp(0.0, 4.0);
                    return Ok(());
                }
                Err(ProcessError::parameter("output_gain must be a float"))
            }
            "enable_bitcrush" => {
                if let ParamValue::Bool(v) = value {
                    self.config.enable_bitcrush = v;
                    return Ok(());
                }
                Err(ProcessError::parameter("enable_bitcrush must be a bool"))
            }
            "enable_sr_reduction" => {
                if let ParamValue::Bool(v) = value {
                    self.config.enable_sr_reduction = v;
                    return Ok(());
                }
                Err(ProcessError::parameter(
                    "enable_sr_reduction must be a bool",
                ))
            }
            "enable_noise" => {
                if let ParamValue::Bool(v) = value {
                    self.config.enable_noise = v;
                    return Ok(());
                }
                Err(ProcessError::parameter("enable_noise must be a bool"))
            }
            _ => Err(ProcessError::parameter(format!(
                "Unknown parameter: {}",
                id
            ))),
        }
    }

    fn id(&self) -> NodeId {
        self.id
    }

    fn set_id(&mut self, id: NodeId) {
        self.id = id;
    }

    fn input_port(&self, index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        self.inputs.get(index)
    }

    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        self.inputs.get_mut(index)
    }

    fn output_port(&self, index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        self.outputs.get(index)
    }

    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        self.outputs.get_mut(index)
    }

    fn control_port(&self, _index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        None
    }

    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        None
    }

    fn state(&self) -> &NodeState<f32, BUF_SIZE> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut NodeState<f32, BUF_SIZE> {
        &mut self.state
    }

    fn num_audio_inputs(&self) -> usize {
        1
    }

    fn num_audio_outputs(&self) -> usize {
        1
    }
}

impl<const BUF_SIZE: usize> Processor<f32, BUF_SIZE> for LofiProcessor<BUF_SIZE> {
    fn process(
        &mut self,
        _clock: &ClockTick,
        audio_inputs: &[&[f32; BUF_SIZE]],
        _control_inputs: &[f32],
        _clock_inputs: &[ClockTick],
        _feedback_inputs: &[&[f32; BUF_SIZE]],
    ) -> ProcessResult<()> {
        if audio_inputs.is_empty() {
            return Ok(());
        }

        let input = audio_inputs[0];
        for (i, sample) in input.iter().enumerate() {
            self.outputs[0].buffer.as_mut_array()[i] = self.process_sample(*sample);
        }

        Ok(())
    }

    fn latency(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_param_id(name: &str) -> ParameterId {
        ParameterId::new(name).unwrap()
    }

    fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn test_lofi_processor_process_basic() {
        let mut processor = LofiProcessor::<64>::new(LofiConfig::default());

        processor
            .set_parameter(&build_param_id("enable_bitcrush"), ParamValue::Bool(false))
            .unwrap();
        processor
            .set_parameter(
                &build_param_id("enable_sr_reduction"),
                ParamValue::Bool(false),
            )
            .unwrap();
        processor
            .set_parameter(&build_param_id("enable_noise"), ParamValue::Bool(false))
            .unwrap();
        processor
            .set_parameter(&build_param_id("dry_wet"), ParamValue::Float(0.0))
            .unwrap();
        processor
            .set_parameter(&build_param_id("output_gain"), ParamValue::Float(0.8))
            .unwrap();
        processor.init(44100.0);

        let mut input = [0.0f32; 64];
        for i in 0..64 {
            input[i] = (i as f32 / 64.0 * std::f32::consts::TAU).sin() * 0.5;
        }

        let clock = ClockTick::new(0, 64, 44100.0);
        processor.process(&clock, &[&input], &[], &[], &[]).unwrap();

        let output = *processor.outputs[0].buffer.as_array();
        for i in 0..64 {
            let expected = input[i] * 0.8;
            assert!(
                approx_eq(output[i], expected, 0.001),
                "Mismatch at {}: got {}, expected {}",
                i,
                output[i],
                expected
            );
        }

        let (_samples, _time) = processor.stats();
    }

    #[test]
    fn test_lofi_processor_with_bitcrush() {
        let mut processor = LofiProcessor::<64>::new(LofiConfig::default());

        processor
            .set_parameter(
                &build_param_id("enable_sr_reduction"),
                ParamValue::Bool(false),
            )
            .unwrap();
        processor
            .set_parameter(&build_param_id("enable_noise"), ParamValue::Bool(false))
            .unwrap();
        processor
            .set_parameter(&build_param_id("enable_bitcrush"), ParamValue::Bool(true))
            .unwrap();
        processor
            .set_parameter(&build_param_id("dry_wet"), ParamValue::Float(0.0))
            .unwrap();
        processor.init(44100.0);

        let input = [0.5f32; 64];
        let clock = ClockTick::new(0, 64, 44100.0);
        processor.process(&clock, &[&input], &[], &[], &[]).unwrap();

        let output = *processor.outputs[0].buffer.as_array();
        for &sample in output.iter() {
            assert!(
                sample >= 0.49 && sample <= 0.51,
                "Bitcrush should not radically change value 0.5"
            );
        }
    }

    #[test]
    fn test_lofi_processor_dry_wet() {
        let mut processor = LofiProcessor::<64>::new(LofiConfig::default());

        processor
            .set_parameter(&build_param_id("enable_bitcrush"), ParamValue::Bool(true))
            .unwrap();
        processor
            .set_parameter(
                &build_param_id("enable_sr_reduction"),
                ParamValue::Bool(false),
            )
            .unwrap();
        processor
            .set_parameter(&build_param_id("enable_noise"), ParamValue::Bool(false))
            .unwrap();
        processor
            .set_parameter(&build_param_id("dry_wet"), ParamValue::Float(0.0))
            .unwrap();
        processor.init(44100.0);

        let input_val = 0.75f32;
        let input = [input_val; 64];
        let clock = ClockTick::new(0, 64, 44100.0);
        processor.process(&clock, &[&input], &[], &[], &[]).unwrap();

        let output = *processor.outputs[0].buffer.as_array();
        assert!(
            approx_eq(output[0], input_val, 0.001),
            "With dry_wet=0, output should equal input"
        );
    }

    #[test]
    fn test_lofi_processor_clear_delay() {
        let mut processor = LofiProcessor::<64>::new(LofiConfig::default());
        processor
            .set_parameter(&build_param_id("enable_bitcrush"), ParamValue::Bool(false))
            .unwrap();
        processor
            .set_parameter(
                &build_param_id("enable_sr_reduction"),
                ParamValue::Bool(false),
            )
            .unwrap();
        processor
            .set_parameter(&build_param_id("enable_noise"), ParamValue::Bool(false))
            .unwrap();
        processor.clear_delay_buffer();
        let input = [0.0f32; 64];
        let clock = ClockTick::new(0, 64, 44100.0);
        processor.process(&clock, &[&input], &[], &[], &[]).unwrap();
        let output = *processor.outputs[0].buffer.as_array();
        for &sample in output.iter() {
            assert!(approx_eq(sample, 0.0, 0.001));
        }
    }

    #[test]
    fn test_lofi_processor_empty_input() {
        let mut processor = LofiProcessor::<64>::new(LofiConfig::default());
        let clock = ClockTick::new(0, 64, 44100.0);
        let result = processor.process(&clock, &[], &[], &[], &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_lofi_processor_parameter_validation() {
        let mut processor = LofiProcessor::<64>::new(LofiConfig::default());

        let result =
            processor.set_parameter(&build_param_id("output_gain"), ParamValue::Float(-1.0));
        assert!(result.is_ok());
        let val = processor
            .get_parameter(&build_param_id("output_gain"))
            .unwrap();
        assert_eq!(val.as_f32(), Some(0.0));

        let result =
            processor.set_parameter(&build_param_id("output_gain"), ParamValue::Float(10.0));
        assert!(result.is_ok());
        let val = processor
            .get_parameter(&build_param_id("output_gain"))
            .unwrap();
        assert_eq!(val.as_f32(), Some(4.0));

        let result =
            processor.set_parameter(&build_param_id("unknown_param"), ParamValue::Float(0.5));
        assert!(result.is_err());
    }

    #[test]
    fn test_lofi_processor_metadata() {
        let processor = LofiProcessor::<64>::new(LofiConfig::default());
        let meta = processor.metadata();
        assert_eq!(meta.audio_inputs, 1);
        assert_eq!(meta.audio_outputs, 1);
        assert_eq!(meta.category, NodeCategory::Processor);
        assert!(!meta.name.is_empty());
    }

    #[test]
    fn test_lofi_processor_for_system() {
        let processor = LofiProcessor::<64>::for_system(ClassicSystem::Nes);
        let meta = processor.metadata();
        assert!(!meta.name.is_empty());
        assert_eq!(meta.audio_inputs, 1);
        assert_eq!(meta.audio_outputs, 1);
        let default_gain = processor
            .get_parameter(&build_param_id("output_gain"))
            .unwrap();
        assert_eq!(default_gain.as_f32(), Some(1.0));
    }

    #[test]
    fn test_lofi_processor_init_reset() {
        let mut processor = LofiProcessor::<64>::new(LofiConfig::default());
        processor.init(48000.0);
        assert!(approx_eq(processor.state.sample_rate, 48000.0, 0.001));

        let input = [0.5f32; 64];
        let clock = ClockTick::new(0, 64, 44100.0);
        processor.process(&clock, &[&input], &[], &[], &[]).unwrap();

        processor.reset();
        assert_eq!(processor.state.sample_pos, 0);
        assert_eq!(processor.state.blocks_processed, 0);
    }
}
