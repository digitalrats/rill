use rill_core::time::ClockTick;
use rill_core::traits::{
    ActionContext, Algorithm, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue,
    ParameterId, Port, SignalNode, Source,
};
use rill_core::Transcendental;
use rill_core::{ProcessError, ProcessResult};
use rill_core_dsp::generators::{Generator, WavetableOscillator};
use std::marker::PhantomData;

/// Wavetable oscillator node with linear or cubic interpolation.
///
/// Generates audio from a fixed wavetable with configurable frequency,
/// amplitude, and interpolation mode.
pub struct WavetableOscNode<T: Transcendental, const BUF_SIZE: usize, const WT_SIZE: usize> {
    osc: WavetableOscillator<T, WT_SIZE>,
    frequency: T,
    amplitude: T,
    cubic: bool,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: Option<NodeState<T, BUF_SIZE>>,
    _phantom: PhantomData<[T; BUF_SIZE]>,
}

impl<T: Transcendental, const BUF_SIZE: usize, const WT_SIZE: usize>
    WavetableOscNode<T, BUF_SIZE, WT_SIZE>
{
    /// Create a new `WavetableOscNode` from an explicit wavetable and frequency.
    pub fn new(table: [T; WT_SIZE], frequency: T) -> Self {
        let freq_f32 = frequency.to_f32();
        let osc = WavetableOscillator::<T, WT_SIZE>::new(table, freq_f32);
        Self {
            osc,
            frequency,
            amplitude: T::from_f32(0.5),
            cubic: false,
            outputs: vec![Port::output(NodeId(0), 0, "signal_out")],
            state: None,
            _phantom: PhantomData,
        }
    }

    /// Create a `WavetableOscNode` using a pre-built sine wavetable.
    pub fn sine(frequency: T) -> Self {
        let freq_f32 = frequency.to_f32();
        let osc = WavetableOscillator::<T, WT_SIZE>::sine(freq_f32);
        Self {
            osc,
            frequency,
            amplitude: T::from_f32(0.5),
            cubic: false,
            outputs: vec![Port::output(NodeId(0), 0, "signal_out")],
            state: None,
            _phantom: PhantomData,
        }
    }

    /// Create a `WavetableOscNode` using a pre-built sawtooth wavetable.
    pub fn saw(frequency: T) -> Self {
        let freq_f32 = frequency.to_f32();
        let osc = WavetableOscillator::<T, WT_SIZE>::saw(freq_f32);
        Self {
            osc,
            frequency,
            amplitude: T::from_f32(0.5),
            cubic: false,
            outputs: vec![Port::output(NodeId(0), 0, "signal_out")],
            state: None,
            _phantom: PhantomData,
        }
    }

    /// Set the output amplitude (clamped to [0, 1]).
    pub fn with_amplitude(mut self, amp: T) -> Self {
        self.amplitude = amp.clamp(T::ZERO, T::from_f32(1.0));
        self
    }

    /// Enable or disable cubic interpolation (linear is the default).
    pub fn with_cubic(mut self, cubic: bool) -> Self {
        self.cubic = cubic;
        self.osc.set_cubic(cubic);
        self
    }

    /// Replace the wavetable data at runtime.
    pub fn set_table(&mut self, table: [T; WT_SIZE]) {
        self.osc.set_table(table);
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

impl<T: Transcendental, const BUF_SIZE: usize, const WT_SIZE: usize> SignalNode<T, BUF_SIZE>
    for WavetableOscNode<T, BUF_SIZE, WT_SIZE>
{
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "WavetableOsc".to_string(),
            type_name: None,
            category: NodeCategory::Source,
            description: "Wavetable oscillator with linear/cubic interpolation".to_string(),
            author: "Rill".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            signal_inputs: 0,
            signal_outputs: 1,
            control_inputs: 0,
            control_outputs: 0,
            clock_inputs: 0,
            clock_outputs: 0,
            feedback_ports: 0,
            parameters: vec![],
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.osc.init(sample_rate);
        self.osc.set_frequency(self.frequency.to_f32());
        self.osc.set_cubic(self.cubic);
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
            "phase" => Some(Self::t_to_param(self.osc.phase())),
            "interpolation" => Some(ParamValue::Choice(
                if self.cubic { "cubic" } else { "linear" }.into(),
            )),
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
            "phase" => {
                if let Some(p) = Self::param_to_t(value) {
                    self.osc.set_phase(p.clamp(T::ZERO, T::from_f32(1.0)));
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "interpolation" => {
                if let ParamValue::Choice(s) = &value {
                    self.cubic = s == "cubic";
                    self.osc.set_cubic(self.cubic);
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
        1
    }
}

impl<T: Transcendental, const BUF_SIZE: usize, const WT_SIZE: usize> Source<T, BUF_SIZE>
    for WavetableOscNode<T, BUF_SIZE, WT_SIZE>
{
    fn generate(
        &mut self,
        clock: &ClockTick,
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
    ) -> ProcessResult<()> {
        let mut temp = [T::ZERO; BUF_SIZE];
        self.osc
            .process(None, &mut temp[..], &ActionContext::new(clock))?;
        for t in temp.iter_mut() {
            *t *= self.amplitude;
        }
        *self.outputs[0].buffer.as_mut_array() = temp;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;

    const WT: usize = 512;

    #[test]
    fn test_create_sine() {
        let osc = WavetableOscNode::<f32, 64, WT>::sine(440.0).with_amplitude(0.7);
        assert!(approx_eq!(f32, osc.frequency, 440.0));
        assert!(approx_eq!(f32, osc.amplitude, 0.7));
    }

    #[test]
    fn test_generation() {
        let mut osc = WavetableOscNode::<f32, 64, WT>::sine(440.0).with_amplitude(0.5);
        osc.init(44100.0);
        let clock = ClockTick::new(0, 64, 44100.0);
        osc.generate(&clock, &[], &[]).unwrap();
        let output = osc.outputs[0].buffer.as_array();
        assert!(
            output.iter().any(|&x| x != 0.0),
            "should produce non-zero output"
        );
        for &s in output.iter() {
            assert!(s >= -0.5 && s <= 0.5, "amplitude within bounds");
        }
    }

    #[test]
    fn test_parameter_handling() {
        let mut osc = WavetableOscNode::<f32, 64, WT>::sine(440.0);
        osc.init(44100.0);

        let freq_id = ParameterId::new("frequency").unwrap();
        osc.set_parameter(&freq_id, ParamValue::Float(880.0))
            .unwrap();
        assert!(approx_eq!(f32, osc.frequency, 880.0));

        let amp_id = ParameterId::new("amplitude").unwrap();
        osc.set_parameter(&amp_id, ParamValue::Float(0.3)).unwrap();
        assert!(approx_eq!(f32, osc.amplitude, 0.3));

        let interp_id = ParameterId::new("interpolation").unwrap();
        osc.set_parameter(&interp_id, ParamValue::Choice("cubic".into()))
            .unwrap();
        assert!(osc.cubic);
    }
}
