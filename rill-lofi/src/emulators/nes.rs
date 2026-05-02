use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;
use rill_core::prelude::*;

#[derive(Clone)]
struct NesPulseChannel {
    duty_cycle: f32,
    frequency: f32,
    volume: f32,
    phase: f32,
    #[allow(dead_code)]
    sweep_enabled: bool,
    #[allow(dead_code)]
    sweep_rate: f32,
}

#[derive(Clone)]
struct NesTriangleChannel {
    frequency: f32,
    volume: f32,
    phase: f32,
    #[allow(dead_code)]
    linear_counter: u8,
}

#[derive(Clone)]
struct NesNoiseChannel {
    mode: NoiseMode,
    frequency: f32,
    volume: f32,
    shift_register: u16,
    tick_counter: f32,
}

#[derive(Clone)]
struct NesDpcmChannel {
    #[allow(dead_code)]
    sample_rate: f32,
    #[allow(dead_code)]
    delta: i8,
    #[allow(dead_code)]
    sample_buffer: Vec<i8>,
    #[allow(dead_code)]
    position: usize,
}

struct NesMixer {
    pulse_mix: f32,
    tnd_mix: f32,
    #[allow(dead_code)]
    output: f32,
}

#[derive(Debug, Clone, Copy)]
enum NoiseMode {
    Short,
    #[allow(dead_code)]
    Long,
}

pub struct NesEmulator<const BUF_SIZE: usize> {
    state: NodeState<f32, BUF_SIZE>,
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<f32, BUF_SIZE>>,

    pulse1: NesPulseChannel,
    pulse2: NesPulseChannel,
    triangle: NesTriangleChannel,
    noise: NesNoiseChannel,
    #[allow(dead_code)]
    dpcm: NesDpcmChannel,
    mixer: NesMixer,
    lofi: LofiProcessor<BUF_SIZE>,
}

impl<const BUF_SIZE: usize> NesEmulator<BUF_SIZE> {
    pub fn new(_sample_rate: f32) -> Self {
        let lofi_config = LofiConfig::for_system(crate::config::ClassicSystem::Nes);
        let id = NodeId(0);
        let state = NodeState::new(_sample_rate);

        let outputs = vec![Port::output(id, 0, "audio_out")];

        Self {
            state,
            id,
            metadata: NodeMetadata {
                name: "NES Sound Chip".to_string(),
                
            type_name: None,category: NodeCategory::Source,
                description: "Nintendo Entertainment System 2A03 sound chip emulation".to_string(),
                author: "Rill Lo-Fi".to_string(),
                version: "1.0".to_string(),
                audio_inputs: 0,
                audio_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![],
            },
            outputs,
            pulse1: NesPulseChannel {
                duty_cycle: 0.25,
                frequency: 440.0,
                volume: 0.5,
                phase: 0.0,
                sweep_enabled: false,
                sweep_rate: 0.0,
            },
            pulse2: NesPulseChannel {
                duty_cycle: 0.125,
                frequency: 660.0,
                volume: 0.3,
                phase: 0.0,
                sweep_enabled: false,
                sweep_rate: 0.0,
            },
            triangle: NesTriangleChannel {
                frequency: 220.0,
                volume: 0.4,
                phase: 0.0,
                linear_counter: 0,
            },
            noise: NesNoiseChannel {
                mode: NoiseMode::Short,
                frequency: 1000.0,
                volume: 0.2,
                shift_register: 1,
                tick_counter: 0.0,
            },
            dpcm: NesDpcmChannel {
                sample_rate: _sample_rate / 2.0,
                delta: 0,
                sample_buffer: Vec::new(),
                position: 0,
            },
            mixer: NesMixer {
                pulse_mix: 0.5,
                tnd_mix: 0.5,
                output: 0.0,
            },
            lofi: LofiProcessor::new(lofi_config),
        }
    }

    fn generate_sample(&mut self) -> f32 {
        let sample_rate = self.state.sample_rate;

        self.pulse1.phase += self.pulse1.frequency / sample_rate;
        if self.pulse1.phase >= 1.0 {
            self.pulse1.phase -= 1.0;
        }

        self.pulse2.phase += self.pulse2.frequency / sample_rate;
        if self.pulse2.phase >= 1.0 {
            self.pulse2.phase -= 1.0;
        }

        self.triangle.phase += self.triangle.frequency / sample_rate;
        if self.triangle.phase >= 1.0 {
            self.triangle.phase -= 1.0;
        }

        let pulse1_val = if self.pulse1.phase < self.pulse1.duty_cycle {
            1.0
        } else {
            -1.0
        } * self.pulse1.volume;

        let pulse2_val = if self.pulse2.phase < self.pulse2.duty_cycle {
            1.0
        } else {
            -1.0
        } * self.pulse2.volume;

        let triangle_val = if self.triangle.phase < 0.5 {
            self.triangle.phase * 4.0 - 1.0
        } else {
            3.0 - self.triangle.phase * 4.0
        } * self.triangle.volume;

        let noise_val = self.generate_noise();
        let dpcm_val = 0.0;

        let pulse_mix = (pulse1_val + pulse2_val) * 0.5;
        let tnd_mix = (triangle_val * 3.0 + noise_val * 2.0 + dpcm_val) / 6.0;

        let mut s = (pulse_mix * self.mixer.pulse_mix + tnd_mix * self.mixer.tnd_mix) * 0.5;
        s = self.lofi.process_sample(s);
        s
    }

    fn generate_noise(&mut self) -> f32 {
        let ticks_per_sample = self.state.sample_rate / self.noise.frequency;

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
}

impl<const BUF_SIZE: usize> SignalNode<f32, BUF_SIZE> for NesEmulator<BUF_SIZE> {
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }

    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }

    fn init(&mut self, sample_rate: f32) {
        self.state = NodeState::new(sample_rate);
        self.lofi.init(sample_rate);
    }

    fn reset(&mut self) {
        self.state.reset();
        self.lofi.reset();
        self.pulse1.phase = 0.0;
        self.pulse2.phase = 0.0;
        self.triangle.phase = 0.0;
        self.noise.shift_register = 1;
        self.noise.tick_counter = 0.0;
    }

    fn get_parameter(&self, _id: &ParameterId) -> Option<ParamValue> {
        None
    }

    fn set_parameter(&mut self, _id: &ParameterId, _value: ParamValue) -> ProcessResult<()> {
        Ok(())
    }

    fn id(&self) -> NodeId {
        self.id
    }

    fn set_id(&mut self, id: NodeId) {
        self.id = id;
    }

    fn input_port(&self, _index: usize) -> Option<&Port<f32, BUF_SIZE>> {
        None
    }
    fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<f32, BUF_SIZE>> {
        None
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
        0
    }
    fn num_audio_outputs(&self) -> usize {
        1
    }
}

impl<const BUF_SIZE: usize> Source<f32, BUF_SIZE> for NesEmulator<BUF_SIZE> {
    fn generate(
        &mut self,
        _clock: &ClockTick,
        _control_inputs: &[f32],
        _clock_inputs: &[ClockTick],
    ) -> ProcessResult<()> {
        for i in 0..BUF_SIZE {
            self.outputs[0].buffer.as_mut_array()[i] = self.generate_sample();
        }
        Ok(())
    }
}
