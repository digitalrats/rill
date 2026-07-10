//! Sawtooth wave oscillator using rill-core-dsp with Transcendental

use rill_core::time::{ClockTick, RenderContext};
use rill_core::traits::{Algorithm, ParamValue, ParameterId, Source};
use rill_core::Transcendental;
use rill_core::{ProcessError, ProcessResult};
use rill_core_dsp::generators::{BasicOscillator, Generator, Waveform};
use std::marker::PhantomData;

/// Sawtooth wave oscillator generic over floating point type
///
/// Uses the optimized BasicOscillator from rill-core-dsp with built-in
/// BLEP anti-aliasing.
pub struct SawOsc<T: Transcendental, const BUF_SIZE: usize> {
    /// Core DSP oscillator
    osc: BasicOscillator<T>,

    /// Base frequency
    frequency: T,

    /// Output amplitude
    amplitude: T,

    /// Audio input ports
    inputs: Vec<Port<T, BUF_SIZE>>,

    /// Audio output ports
    outputs: Vec<Port<T, BUF_SIZE>>,

    /// Control ports
    controls: Vec<Port<T, BUF_SIZE>>,

    /// Node state
    // (removed legacy field)

    /// Phantom data
    _phantom: PhantomData<[T; BUF_SIZE]>,
}

impl<T: Transcendental, const BUF_SIZE: usize> SawOsc<T, BUF_SIZE> {
    /// Create new sawtooth oscillator
    pub fn new() -> Self {
        let osc = BasicOscillator::new(Waveform::Saw, 440.0, T::from_f32(1.0));

        Self {
            osc,
            frequency: T::from_f32(440.0),
            amplitude: T::from_f32(0.5),
            inputs: Vec::new(),
            outputs: vec![Port::output(NodeId(0), 0, "signal_out")],
            controls: Vec::new(),
            state: None,
            _phantom: PhantomData,
        }
    }

    /// Set frequency
    pub fn with_frequency(mut self, freq: T) -> Self {
        self.frequency = freq.max(T::from_f32(0.1)).min(T::from_f32(20000.0));
        self.osc.set_frequency(self.frequency.to_f32());
        self
    }

    /// Set amplitude
    pub fn with_amplitude(mut self, amp: T) -> Self {
        self.amplitude = amp.clamp(T::ZERO, T::from_f32(1.0));
        self
    }

    /// Convert ParamValue to T
    fn param_to_t(value: ParamValue) -> Option<T> {
        match value {
            ParamValue::Float(f) => Some(T::from_f32(f)),
            ParamValue::Int(i) => Some(T::from_f32(i as f32)),
            _ => None,
        }
    }

    /// Convert T to ParamValue
    fn t_to_param(value: T) -> ParamValue {
        ParamValue::Float(value.to_f32())
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for SawOsc<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}


    fn init(&mut self, sample_rate: f32) {
        self.osc.init(sample_rate);
        self.osc.set_frequency(self.frequency.to_f32());
        self.state = Some(NodeState::new(sample_rate));
    }

    fn reset(&mut self) {
        self.osc.reset();
        if let Some(state) = &mut self.state {
            state.reset();
        }
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "frequency" => Some(Self::t_to_param(self.frequency)),
            "amplitude" => Some(Self::t_to_param(self.amplitude)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match id.as_str() {
            "frequency" => {
                if let Some(f) = Self::param_to_t(value) {
                    self.frequency = f.max(T::from_f32(0.1)).min(T::from_f32(20000.0));
                    self.osc.set_frequency(self.frequency.to_f32());
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

    fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.inputs.get(index)
    }

    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.inputs.get_mut(index)
    }

    fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.outputs.get(index)
    }

    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.outputs.get_mut(index)
    }

    fn control_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.controls.get(index)
    }

    fn control_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.controls.get_mut(index)
    }

        self.state.as_ref().unwrap()
    }

        self.state.as_mut().unwrap()
    }

    fn num_signal_inputs(&self) -> usize {
        0
    }

    fn num_signal_outputs(&self) -> usize {
        1
    }

        &mut self,
        _ctx: &RenderContext,
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _tick: &ClockTick,
    ) -> ProcessResult<()> {
        let out = self.outputs[0].write();
        self.osc.set_frequency(self.frequency.to_f32());
        self.osc.process(None, &mut out[..])?;
        for o in out.iter_mut() {
            *o *= self.amplitude;
        }
        self.state.as_mut().unwrap().advance();
        Ok(())
    }
