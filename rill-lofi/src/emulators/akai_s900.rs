use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;
use rill_core::prelude::*;

/// Emulates the Akai S900 hardware sampler, including its 12-bit DAC, analog
/// filters, and sample interpolation with pitch shifting and loop support.
pub struct AkaiS900Emulator<const BUF_SIZE: usize> {
    state: NodeState<f32, BUF_SIZE>,
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<f32, BUF_SIZE>>,

    buffer: Vec<f32>,
    position: f32,
    pitch: f32,
    loop_enabled: bool,
    loop_start: usize,
    loop_end: usize,
    lofi: LofiProcessor<BUF_SIZE>,
}

impl<const BUF_SIZE: usize> AkaiS900Emulator<BUF_SIZE> {
    /// Creates a new `AkaiS900Emulator` configured with the S900's 12-bit
    /// nonlinear DAC and default pitch / looping parameters.
    pub fn new(_sample_rate: f32) -> Self {
        let lofi_config = LofiConfig::for_system(crate::config::ClassicSystem::AkaiS900);
        let id = NodeId(0);
        let state = NodeState::new(_sample_rate);

        let outputs = vec![Port::output(id, 0, "signal_out")];

        Self {
            state,
            id,
            metadata: NodeMetadata {
                name: "Akai S900".to_string(),

                type_name: None,
                category: NodeCategory::Source,
                description: "Akai S900 sampler emulation".to_string(),
                author: "Rill Lo-Fi".to_string(),
                version: "1.0".to_string(),
                signal_inputs: 0,
                signal_outputs: 1,
                control_inputs: 0,
                control_outputs: 0,
                clock_inputs: 0,
                clock_outputs: 0,
                feedback_ports: 0,
                parameters: vec![
                    ParamMetadata::new("pitch", ParamType::Float, ParamValue::Float(1.0))
                        .with_description("Playback pitch")
                        .with_range(0.1, 4.0, 0.01)
                        .with_unit("x"),
                    ParamMetadata::new("loop_enabled", ParamType::Bool, ParamValue::Bool(false))
                        .with_description("Enable sample looping"),
                ],
            },
            outputs,
            buffer: Vec::new(),
            position: 0.0,
            pitch: 1.0,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            lofi: LofiProcessor::new(lofi_config),
        }
    }

    /// Loads a sample buffer into the emulator and sets the loop end point to
    /// the buffer length.
    pub fn load_sample(&mut self, samples: &[f32]) {
        self.buffer = samples.to_vec();
        self.loop_end = samples.len();
    }

    /// Sets the playback pitch multiplier, clamped to the valid range `[0.1, 4.0]`.
    pub fn set_pitch(&mut self, pitch: f32) {
        self.pitch = pitch.clamp(0.1, 4.0);
    }

    fn generate_sample(&mut self) -> f32 {
        if self.buffer.is_empty() {
            return 0.0;
        }

        if (self.position as usize) >= self.buffer.len() {
            return 0.0;
        }

        let sample = if (self.position as usize) < self.buffer.len() - 1 {
            let idx = self.position.floor() as usize;
            let frac = self.position.fract();
            self.buffer[idx] * (1.0 - frac) + self.buffer[idx + 1] * frac
        } else {
            self.buffer[self.position as usize]
        };

        let processed = self.lofi.process_sample(sample);

        self.position += self.pitch;

        if self.loop_enabled && (self.position as usize) >= self.loop_end {
            self.position = self.loop_start as f32 + (self.position - self.loop_end as f32);
        }

        processed
    }
}

impl<const BUF_SIZE: usize> Node<f32, BUF_SIZE> for AkaiS900Emulator<BUF_SIZE> {
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
        self.position = 0.0;
        self.lofi.reset();
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "pitch" => Some(ParamValue::Float(self.pitch)),
            "loop_enabled" => Some(ParamValue::Bool(self.loop_enabled)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match id.as_str() {
            "pitch" => {
                if let ParamValue::Float(v) = value {
                    self.pitch = v.clamp(0.1, 4.0);
                    Ok(())
                } else {
                    Err(ProcessError::parameter("pitch must be a float"))
                }
            }
            "loop_enabled" => {
                if let ParamValue::Bool(v) = value {
                    self.loop_enabled = v;
                    Ok(())
                } else {
                    Err(ProcessError::parameter("loop_enabled must be a bool"))
                }
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

    fn num_signal_inputs(&self) -> usize {
        0
    }
    fn num_signal_outputs(&self) -> usize {
        1
    }
}

impl<const BUF_SIZE: usize> Source<f32, BUF_SIZE> for AkaiS900Emulator<BUF_SIZE> {
    fn generate(
        &mut self,
        _ctx: &RenderContext,
        _control_inputs: &[f32],
        _clock_inputs: &[RenderContext],
        _tick: &ClockTick,
    ) -> ProcessResult<()> {
        for i in 0..BUF_SIZE {
            self.outputs[0].write()[i] = self.generate_sample();
        }
        Ok(())
    }
}
