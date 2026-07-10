// rill-fft/src/nodes/convolver_node.rs
//! Convolution node for the signal graph.
//!
//! Wraps `PartitionedConvolver` as a `Processor` node, enabling impulse response
//! (IR) convolution in any signal graph.

use rill_core::{
    math::Transcendental, ParamValue, ParameterId, ProcessError, ProcessResult, RenderContext,
};

use crate::partitioned_conv::PartitionedConvolver;

/// Convolution node — applies an impulse response via partitioned FFT convolution.
///
/// # Parameters
///
/// - `ir_gain` — impulse response gain (0.0–4.0), default 1.0
/// - `mix` — dry/wet mix (0.0–1.0), default 1.0
///
/// # IR loading
///
/// The impulse response is set via `set_ir(&[T])`. To load from a WAV file,
/// use `rill-sampler` or any WAV reader, then pass the samples.
pub struct ConvolverNode<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
    controls: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    convolver: PartitionedConvolver<T, BUF_SIZE>,
    ir_gain: f32,
    mix: f32,
    ir_loaded: bool,
}

impl<T: Transcendental, const BUF_SIZE: usize> ConvolverNode<T, BUF_SIZE> {
    /// Create a new convolution node.
    ///
    /// `ir_len` is the expected impulse response length in samples.
    /// The convolver pre-allocates buffers based on this size.
    pub fn new(ir_len: usize, sample_rate: f32) -> Self {
        let metadata = NodeMetadata::new("Convolver", NodeCategory::Processor);
        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        inputs.push(Port::input(NodeId(0), 0, "signal_in"));
        outputs.push(Port::output(NodeId(0), 0, "signal_out"));

        let convolver = PartitionedConvolver::new(ir_len);

        Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
            convolver,
            ir_gain: 1.0,
            mix: 1.0,
            ir_loaded: false,
        }
    }

    /// Set the impulse response from a slice of samples.
    pub fn set_ir(&mut self, ir: &[T]) {
        self.convolver.set_ir(ir);
        self.ir_loaded = true;
    }

    /// Return whether an impulse response has been loaded.
    pub fn ir_loaded(&self) -> bool {
        self.ir_loaded
    }

    /// Set impulse response gain.
    pub fn set_ir_gain(&mut self, gain: f32) {
        self.ir_gain = gain.clamp(0.0, 4.0);
    }

    /// Set dry/wet mix.
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }
}

    where
        Self: 'static + Sized,
    {
        rill_core::NodeTypeId::of::<Self>()
    }

    fn id(&self) -> NodeId {
        self.id
    }

    fn set_id(&mut self, id: NodeId) {
        self.id = id;
    }

    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }

    fn init(&mut self, sample_rate: f32) {
        self.state = NodeState::new(sample_rate);
    }

    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        let name = id.as_str();
        match name {
            "ir_gain" => Some(ParamValue::Float(self.ir_gain)),
            "mix" => Some(ParamValue::Float(self.mix)),
            "ir_loaded" => Some(ParamValue::Bool(self.ir_loaded)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if let Some(v) = value.as_f32() {
            match name {
                "ir_gain" => {
                    self.set_ir_gain(v);
                    Ok(())
                }
                "mix" => {
                    self.set_mix(v);
                    Ok(())
                }
                _ => Err(ProcessError::parameter(format!(
                    "Unknown parameter: {name}"
                ))),
            }
        } else {
            Err(ProcessError::parameter("Expected float value"))
        }
    }

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

    fn num_inputs(&self) -> usize {
        self.inputs.len()
    }

    fn num_outputs(&self) -> usize {
        self.outputs.len()
    }

    fn num_signal_inputs(&self) -> usize {
        self.inputs.len()
    }

    fn num_signal_outputs(&self) -> usize {
        self.outputs.len()
    }

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
    for ConvolverNode<T, BUF_SIZE>
{
    fn process(
        &mut self,
        _ctx: &RenderContext,
        _signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        let inp = self.inputs[0].read();
        let out = self.outputs[0].write();

        if !self.ir_loaded {
            // Passthrough when no IR is loaded
            out.copy_from_slice(inp);
        } else {
            let gain = T::from_f32(self.ir_gain);
            let mix = T::from_f32(self.mix);
            let one_minus_mix = T::ONE - mix;

            // Run the convolver — it expects slices of BUF_SIZE
            self.convolver.process(inp, out);

            // Apply gain and mix
            for i in 0..BUF_SIZE {
                let wet = out[i] * gain;
                out[i] = inp[i] * one_minus_mix + wet * mix;
            }
        }

        self.state.advance();
        Ok(())
    }

    fn latency(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convolver_node_passthrough_no_ir() {
        let mut node = ConvolverNode::<f32, 64>::new(1024, 44100.0);

        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        node.inputs[0].write().copy_from_slice(&input);

        let ctx = RenderContext::new(0, 0, 44100.0);
        node.process(&ctx, &[], &[], &[], &[]).unwrap();

        let output = node.outputs[0].read();
        for (i, o) in input.iter().zip(output.iter()) {
            assert!((i - o).abs() < 1e-5);
        }
    }

    #[test]
    fn test_convolver_node_unit_ir_passthrough() {
        let mut node = ConvolverNode::<f32, 64>::new(4, 44100.0);
        node.set_ir(&[1.0, 0.0, 0.0, 0.0]);
        node.set_mix(1.0);

        let input: Vec<f32> = (0..64).map(|i| (i as f32 * 0.1).sin()).collect();
        node.inputs[0].write().copy_from_slice(&input);

        let ctx = RenderContext::new(0, 0, 44100.0);
        node.process(&ctx, &[], &[], &[], &[]).unwrap();

        let output = node.outputs[0].read();
        for (i, o) in input.iter().zip(output.iter()) {
            assert!((i - o).abs() < 5e-3, "expected {i}, got {o}");
        }
    }

    #[test]
    fn test_convolver_node_parameters() {
        let mut node = ConvolverNode::<f32, 64>::new(1024, 44100.0);

        let ir_gain_id = ParameterId::new("ir_gain").unwrap();
        let mix_id = ParameterId::new("mix").unwrap();
        let ir_loaded_id = ParameterId::new("ir_loaded").unwrap();

        assert_eq!(
            node.get_parameter(&ir_gain_id),
            Some(ParamValue::Float(1.0))
        );
        assert_eq!(node.get_parameter(&mix_id), Some(ParamValue::Float(1.0)));
        assert_eq!(
            node.get_parameter(&ir_loaded_id),
            Some(ParamValue::Bool(false))
        );

        node.set_parameter(&ir_gain_id, ParamValue::Float(2.0))
            .unwrap();
        assert_eq!(
            node.get_parameter(&ir_gain_id),
            Some(ParamValue::Float(2.0))
        );
    }
}
