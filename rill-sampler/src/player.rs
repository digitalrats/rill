use rill_core::time::ClockTick;
use rill_core::traits::{
    Algorithm, SignalNode, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue, ParameterId,
    Port, Source,
};
use rill_core::Transcendental;
use rill_core::{ProcessError, ProcessResult};
use rill_core_dsp::generators::{Generator, LoopMode, SamplePlayer};
use std::marker::PhantomData;

use crate::buffer::SampleBuffer;

/// Sample-playback source node with stereo support.
///
/// # Parameters (all automatable via patchbay)
///
/// | Name | Type | Range | Description |
/// |---|---|---|---|
/// | `"gate"` | Bool | – | Start / stop playback |
/// | `"rate"` | Float | 0.0–4.0 | Playback speed ratio |
/// | `"loop_mode"` | Choice | oneshot/forward/pingpong | Loop behaviour |
/// | `"start"` | Float | 0.0–1.0 | Loop start (normalised) |
/// | `"end"` | Float | 0.0–1.0 | Loop end (normalised) |
/// | `"amplitude"` | Float | 0.0–1.0 | Output gain |
/// | `"interpolation"` | Choice | linear/cubic | Interpolation mode |
/// | `"position"` | Float | 0.0–1.0 | Current position **(read-only)** |
///
/// # Output ports
/// - Port 0: left channel
/// - Port 1: right channel (only present when a stereo sample is loaded)
pub struct SamplePlayerNode<T: Transcendental, const BUF_SIZE: usize> {
    left: SamplePlayer<T>,
    right: Option<SamplePlayer<T>>,
    gate: bool,
    amplitude: T,
    rate: f64,
    loop_mode: LoopMode,
    loop_start: f64,
    loop_end: f64,
    cubic: bool,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: Option<NodeState<T, BUF_SIZE>>,
    _phantom: PhantomData<[T; BUF_SIZE]>,
}

impl<T: Transcendental, const BUF_SIZE: usize> SamplePlayerNode<T, BUF_SIZE> {
    /// Create a new node with an empty sample buffer.
    pub fn new() -> Self {
        Self {
            left: SamplePlayer::new(Vec::new()),
            right: None,
            gate: false,
            amplitude: T::from_f32(1.0),
            rate: 1.0,
            loop_mode: LoopMode::OneShot,
            loop_start: 0.0,
            loop_end: 0.0,
            cubic: false,
            outputs: vec![Port::output(NodeId(0), 0, "left")],
            state: None,
            _phantom: PhantomData,
        }
    }

    /// Load a sample buffer into the node.
    pub fn load(&mut self, sample: SampleBuffer<T>) {
        let len = sample.len() as f64;
        self.loop_end = len;
        self.loop_start = 0.0;

        self.left.set_buffer(sample.data);
        self.left
            .set_loop_start(self.loop_start);
        self.left.set_loop_end(self.loop_end);
        self.left
            .set_loop_mode(self.loop_mode);
        self.left
            .set_playback_rate(self.rate);
        self.left.set_cubic(self.cubic);

        if let Some(right_data) = sample.right {
            let mut right_player = SamplePlayer::new(right_data);
            right_player.set_loop_start(self.loop_start);
            right_player.set_loop_end(self.loop_end);
            right_player.set_loop_mode(self.loop_mode);
            right_player.set_playback_rate(self.rate);
            right_player.set_cubic(self.cubic);
            self.right = Some(right_player);

            if self.outputs.len() < 2 {
                self.outputs
                    .push(Port::output(NodeId(0), 1, "right"));
            }
        } else {
            self.right = None;
            self.outputs.truncate(1);
        }
    }

    /// Start / stop playback.
    pub fn play(&mut self) {
        self.gate = true;
        self.left.set_gate(true);
        if let Some(ref mut r) = self.right {
            r.set_gate(true);
        }
    }

    /// Stop playback (sets gate to false).
    pub fn stop(&mut self) {
        self.gate = false;
        self.left.set_gate(false);
        if let Some(ref mut r) = self.right {
            r.set_gate(false);
        }
    }

    fn param_to_t(value: ParamValue) -> Option<T> {
        match value {
            ParamValue::Float(f) => Some(T::from_f32(f)),
            ParamValue::Int(i) => Some(T::from_f32(i as f32)),
            _ => None,
        }
    }

    fn t_to_param(value: T) -> ParamValue {
        ParamValue::Float(value.to_f32())
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for SamplePlayerNode<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE>
    for SamplePlayerNode<T, BUF_SIZE>
{
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "SamplePlayer".to_string(),
            type_name: None,
            category: NodeCategory::Source,
            description: "Sample playback node with loop modes and stereo".to_string(),
            author: "Rill".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            signal_inputs: 0,
            signal_outputs: if self.right.is_some() { 2 } else { 1 },
            control_inputs: 0,
            control_outputs: 0,
            clock_inputs: 0,
            clock_outputs: 0,
            feedback_ports: 0,
            parameters: vec![],
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.left.init(sample_rate);
        if let Some(ref mut r) = self.right {
            r.init(sample_rate);
        }
        self.state = Some(NodeState::new(sample_rate));
    }

    fn reset(&mut self) {
        self.left.reset();
        if let Some(ref mut r) = self.right {
            r.reset();
        }
        self.gate = false;
        if let Some(state) = &mut self.state {
            state.reset();
        }
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "gate" => Some(ParamValue::Bool(self.gate)),
            "rate" => Some(ParamValue::Float(self.rate as f32)),
            "loop_mode" => {
                let s = match self.loop_mode {
                    LoopMode::OneShot => "oneshot",
                    LoopMode::Forward => "forward",
                    LoopMode::PingPong => "pingpong",
                };
                Some(ParamValue::Choice(s.into()))
            }
            "start" => {
                let len = self.left.len().max(1) as f64;
                Some(ParamValue::Float((self.loop_start / len) as f32))
            }
            "end" => {
                let len = self.left.len().max(1) as f64;
                Some(ParamValue::Float((self.loop_end / len) as f32))
            }
            "amplitude" => Some(Self::t_to_param(self.amplitude)),
            "interpolation" => Some(ParamValue::Choice(
                if self.cubic { "cubic" } else { "linear" }.into(),
            )),
            "position" => Some(ParamValue::Float(self.left.phase().to_f32())),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let len = self.left.len().max(1) as f64;
        match id.as_str() {
            "gate" => {
                if let ParamValue::Bool(b) = value {
                    self.gate = b;
                    self.left.set_gate(b);
                    if let Some(ref mut r) = self.right {
                        r.set_gate(b);
                    }
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected bool".into()))
                }
            }
            "rate" => {
                if let Some(r) = Self::param_to_t(value) {
                    self.rate = r.to_f64().clamp(0.0, 4.0);
                    self.left.set_playback_rate(self.rate);
                    if let Some(ref mut rp) = self.right {
                        rp.set_playback_rate(self.rate);
                    }
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "loop_mode" => {
                if let ParamValue::Choice(s) = &value {
                    self.loop_mode = match s.as_str() {
                        "forward" => LoopMode::Forward,
                        "pingpong" => LoopMode::PingPong,
                        _ => LoopMode::OneShot,
                    };
                    self.left.set_loop_mode(self.loop_mode);
                    if let Some(ref mut r) = self.right {
                        r.set_loop_mode(self.loop_mode);
                    }
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected choice".into()))
                }
            }
            "start" => {
                if let Some(s) = Self::param_to_t(value) {
                    self.loop_start = (s.to_f64() * len).clamp(0.0, self.loop_end);
                    self.left.set_loop_start(self.loop_start);
                    if let Some(ref mut r) = self.right {
                        r.set_loop_start(self.loop_start);
                    }
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "end" => {
                if let Some(e) = Self::param_to_t(value) {
                    self.loop_end = (e.to_f64() * len).clamp(self.loop_start, len);
                    self.left.set_loop_end(self.loop_end);
                    if let Some(ref mut r) = self.right {
                        r.set_loop_end(self.loop_end);
                    }
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "amplitude" => {
                if let Some(a) = Self::param_to_t(value) {
                    self.amplitude = a.clamp(T::ZERO, T::from_f32(1.0));
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "interpolation" => {
                if let ParamValue::Choice(s) = &value {
                    self.cubic = s == "cubic";
                    self.left.set_cubic(self.cubic);
                    if let Some(ref mut r) = self.right {
                        r.set_cubic(self.cubic);
                    }
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected choice".into()))
                }
            }
            _ => Err(ProcessError::Parameter(format!(
                "Unknown parameter: {}",
                id
            ))),
        }
    }

    fn id(&self) -> NodeId {
        NodeId(0)
    }

    fn set_id(&mut self, _id: NodeId) {}

    fn input_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }

    fn input_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }

    fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.outputs.get(index)
    }

    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.outputs.get_mut(index)
    }

    fn control_port(&self, _index: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }

    fn control_port_mut(&mut self, _index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        self.state.as_ref().unwrap()
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        self.state.as_mut().unwrap()
    }

    fn num_signal_inputs(&self) -> usize {
        0
    }

    fn num_signal_outputs(&self) -> usize {
        self.outputs.len()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE>
    for SamplePlayerNode<T, BUF_SIZE>
{
    fn generate(
        &mut self,
        clock: &ClockTick,
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
    ) -> ProcessResult<()> {
        let amp = self.amplitude;

        let mut temp = [T::ZERO; BUF_SIZE];
        self.left
            .process(None, &mut temp[..], &rill_core::traits::ActionContext::new(clock))?;
        if amp != T::from_f32(1.0) {
            for s in temp.iter_mut() {
                *s *= amp;
            }
        }
        *self.outputs[0].buffer.as_mut_array() = temp;

        if let Some(ref mut right_player) = self.right {
            let mut right_temp = [T::ZERO; BUF_SIZE];
            right_player
                .process(None, &mut right_temp[..], &rill_core::traits::ActionContext::new(clock))?;
            if amp != T::from_f32(1.0) {
                for s in right_temp.iter_mut() {
                    *s *= amp;
                }
            }
            if self.outputs.len() > 1 {
                *self.outputs[1].buffer.as_mut_array() = right_temp;
            }
        }

        Ok(())
    }
}
