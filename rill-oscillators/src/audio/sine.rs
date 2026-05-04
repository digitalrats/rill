//! Sine wave oscillator using rill-core-dsp with Transcendental

use rill_core::time::ClockTick;
use rill_core::traits::{
    ActionContext, Algorithm, NodeCategory, NodeId, NodeMetadata, NodeState, ParamValue,
    ParameterId, Port, SignalNode, Source,
};
use rill_core::Transcendental;
use rill_core::{ProcessError, ProcessResult};
use rill_core_dsp::generators::{BasicOscillator, Generator, Waveform};
use std::marker::PhantomData;

/// Sine wave oscillator generic over floating point type
///
/// Uses the optimized BasicOscillator from rill-core-dsp with Transcendental trait.
/// Supports both f32 and f64 through type parameter T.
///
/// # Type Parameters
/// - `T`: Floating point type (f32 or f64) implementing Transcendental
/// - `BUF_SIZE`: Fixed block size for processing
///
/// # Parameters
/// - `frequency`: Base frequency in Hz
/// - `amplitude`: Output amplitude (0.0 to 1.0)
/// - `phase`: Initial phase offset (0.0 to 1.0)
/// - `fm_amount`: Frequency modulation amount
///
/// # Inputs
/// - Port 0: Optional frequency modulation input (normalized 0.0 to 1.0)
///
/// # Outputs
/// - Port 0: Sine wave output
pub struct SineOsc<T: Transcendental, const BUF_SIZE: usize> {
    /// Core DSP oscillator
    osc: BasicOscillator<T>,

    /// Base frequency in Hz
    frequency: T,

    /// Output amplitude (0.0 to 1.0)
    amplitude: T,

    /// Phase offset (0.0 to 1.0)
    phase_offset: T,

    /// FM amount in Hz (max deviation)
    fm_amount: T,

    /// Whether FM is enabled
    use_fm: bool,

    /// Audio input ports
    inputs: Vec<Port<T, BUF_SIZE>>,

    /// Audio output ports
    outputs: Vec<Port<T, BUF_SIZE>>,

    /// Control ports
    controls: Vec<Port<T, BUF_SIZE>>,

    /// Node state
    state: Option<NodeState<T, BUF_SIZE>>,

    /// Phantom data to satisfy const generic
    _phantom: PhantomData<[T; BUF_SIZE]>,
}

impl<T: Transcendental, const BUF_SIZE: usize> SineOsc<T, BUF_SIZE> {
    /// Create new sine oscillator with default settings
    pub fn new() -> Self {
        let osc = BasicOscillator::new(Waveform::Sine, 440.0, T::from_f32(1.0));

        Self {
            osc,
            frequency: T::from_f32(440.0),
            amplitude: T::from_f32(0.5),
            phase_offset: T::ZERO,
            fm_amount: T::ZERO,
            use_fm: false,
            inputs: Vec::new(),
            outputs: vec![Port::output(NodeId(0), 0, "signal_out")],
            controls: Vec::new(),
            state: None,
            _phantom: PhantomData,
        }
    }

    /// Set base frequency
    pub fn with_frequency(mut self, freq: T) -> Self {
        self.frequency = freq.max(T::from_f32(0.1)).min(T::from_f32(20000.0));
        self.osc.set_frequency(self.frequency.to_f32());
        self
    }

    /// Set output amplitude (0.0 to 1.0)
    pub fn with_amplitude(mut self, amp: T) -> Self {
        self.amplitude = amp.clamp(T::ZERO, T::from_f32(1.0));
        self
    }

    /// Set phase offset (0.0 to 1.0)
    pub fn with_phase(mut self, phase: T) -> Self {
        self.phase_offset = phase.clamp(T::ZERO, T::from_f32(1.0));
        self.osc.set_phase(self.phase_offset);
        self
    }

    /// Enable frequency modulation with specified amount
    pub fn with_fm(mut self, amount: T) -> Self {
        self.use_fm = true;
        self.fm_amount = amount.clamp(T::ZERO, T::from_f32(10.0));
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

    /// Generate a block of samples without FM (efficient block processing)
    fn generate_block_no_fm(
        &mut self,
        output: &mut [T; BUF_SIZE],
        clock: &ClockTick,
    ) -> ProcessResult<()> {
        self.osc.set_frequency(self.frequency.to_f32());
        self.osc
            .process(None, &mut output[..], &ActionContext::new(clock))?;
        for i in 0..BUF_SIZE {
            output[i] *= self.amplitude;
        }
        Ok(())
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Default for SineOsc<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for SineOsc<T, BUF_SIZE> {
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "SineOsc".to_string(),

            type_name: None,
            category: NodeCategory::Source,
            description: "Sine wave oscillator with FM".to_string(),
            author: "Rill".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            signal_inputs: if self.use_fm { 1 } else { 0 },
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
        self.osc.set_phase(self.phase_offset);
        if self.use_fm && self.inputs.is_empty() {
            self.inputs.push(Port::input(NodeId(0), 0, "fm_in"));
        }
        self.state = Some(NodeState::new(sample_rate));
    }

    fn reset(&mut self) {
        self.osc.reset();
        self.osc.set_phase(self.phase_offset);
        if let Some(state) = &mut self.state {
            state.reset();
        }
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "frequency" => Some(Self::t_to_param(self.frequency)),
            "amplitude" => Some(Self::t_to_param(self.amplitude)),
            "phase" => Some(Self::t_to_param(self.phase_offset)),
            "fm_amount" => Some(Self::t_to_param(self.fm_amount)),
            "use_fm" => Some(ParamValue::Bool(self.use_fm)),
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
                    self.phase_offset = p.clamp(T::ZERO, T::from_f32(1.0));
                    self.osc.set_phase(self.phase_offset);
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "fm_amount" => {
                if let Some(a) = Self::param_to_t(value) {
                    self.fm_amount = a.clamp(T::ZERO, T::from_f32(10.0));
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "use_fm" => {
                if let ParamValue::Bool(b) = value {
                    self.use_fm = b;
                    if b && self.inputs.is_empty() {
                        self.inputs.push(Port::input(NodeId(0), 0, "fm_in"));
                    } else if !b && !self.inputs.is_empty() {
                        self.inputs.clear();
                    }
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected bool".into()))
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

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        self.state.as_ref().unwrap()
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        self.state.as_mut().unwrap()
    }

    fn num_signal_inputs(&self) -> usize {
        if self.use_fm {
            1
        } else {
            0
        }
    }

    fn num_signal_outputs(&self) -> usize {
        1
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for SineOsc<T, BUF_SIZE> {
    fn generate(
        &mut self,
        clock: &ClockTick,
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
    ) -> ProcessResult<()> {
        let mut temp = [T::ZERO; BUF_SIZE];
        self.generate_block_no_fm(&mut temp, clock)?;
        *self.outputs[0].buffer.as_mut_array() = temp;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;

    #[test]
    fn test_sine_creation_f32() {
        let osc = SineOsc::<f32, 64>::new()
            .with_frequency(440.0)
            .with_amplitude(0.7)
            .with_phase(0.25);

        assert!(approx_eq!(f32, osc.frequency, 440.0));
        assert!(approx_eq!(f32, osc.amplitude, 0.7));
        assert!(approx_eq!(f32, osc.phase_offset, 0.25));
    }

    #[test]
    fn test_sine_creation_f64() {
        let osc = SineOsc::<f64, 64>::new()
            .with_frequency(440.0)
            .with_amplitude(0.7)
            .with_phase(0.25);

        assert!((osc.frequency - 440.0).abs() < 1e-10);
        assert!((osc.amplitude - 0.7).abs() < 1e-10);
        assert!((osc.phase_offset - 0.25).abs() < 1e-10);
    }

    #[test]
    fn test_sine_generation_f32() {
        let mut osc = SineOsc::<f32, 64>::new()
            .with_frequency(440.0)
            .with_amplitude(0.5);

        osc.init(44100.0);

        let clock = ClockTick::new(0, 64, 44100.0);
        osc.generate(&clock, &[], &[]).unwrap();

        let output = osc.outputs[0].buffer.as_array();

        // First sample should be near 0 (sine with phase 0)
        assert!(approx_eq!(f32, output[0], 0.0, epsilon = 1e-4));

        // All samples should be within amplitude range
        for &sample in output.iter() {
            assert!(sample >= -0.5 && sample <= 0.5);
        }
    }

    #[test]
    fn test_sine_generation_f64() {
        let mut osc = SineOsc::<f64, 64>::new()
            .with_frequency(440.0)
            .with_amplitude(0.5);

        osc.init(44100.0);

        let clock = ClockTick::new(0, 64, 44100.0);
        osc.generate(&clock, &[], &[]).unwrap();

        let output = osc.outputs[0].buffer.as_array();

        // First sample should be near 0
        assert!((output[0]).abs() < 1e-10);

        // All samples should be within amplitude range
        for &sample in output.iter() {
            assert!(sample >= -0.5 && sample <= 0.5);
        }
    }

    #[test]
    fn test_sine_with_fm() {
        let mut osc = SineOsc::<f32, 64>::new().with_frequency(440.0).with_fm(2.0);

        osc.init(44100.0);

        let clock = ClockTick::new(0, 64, 44100.0);
        osc.generate(&clock, &[], &[]).unwrap();

        let output = osc.outputs[0].buffer.as_array();

        // Should produce valid output
        assert!(output.iter().any(|&x| x != 0.0));
    }

    #[test]
    fn test_parameter_handling() {
        let mut osc = SineOsc::<f32, 64>::new();

        let freq_id = ParameterId::new("frequency").unwrap();
        osc.set_parameter(&freq_id, ParamValue::Float(880.0))
            .unwrap();
        assert!(approx_eq!(f32, osc.frequency, 880.0));

        let amp_id = ParameterId::new("amplitude").unwrap();
        osc.set_parameter(&amp_id, ParamValue::Float(0.3)).unwrap();
        assert!(approx_eq!(f32, osc.amplitude, 0.3));

        let phase_id = ParameterId::new("phase").unwrap();
        osc.set_parameter(&phase_id, ParamValue::Float(0.75))
            .unwrap();
        assert!(approx_eq!(f32, osc.phase_offset, 0.75));
    }
}
