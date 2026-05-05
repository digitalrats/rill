use rill_core::{
    buffer::{BufferRegistry, TapeLoop},
    math::Transcendental,
    traits::{NodeCategory, NodeMetadata, NodeState, SignalNode, Source},
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessError, ProcessResult,
};

/// Read head — pure tape reader. Reads from the shared [`TapeLoop`] at a
/// fixed delay. Mono output. Level and pan are handled by a downstream
/// SumNode with per-channel gains.
///
/// The tape loop is obtained through the graph's resource registry during
/// node initialization.
///
/// # Signal ports
/// - 1 audio output (mono), no inputs
///
/// # Parameters
/// - `delay` (0.01 – 2.0 s)
pub struct ReadHead<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    tape: *const TapeLoop<T>,
    resource_name: String,
    delay: f32,
    sample_rate: f32,
}

// Raw pointer — safe, graph is single-threaded.
#[allow(unsafe_code)]
unsafe impl<T: Transcendental, const B: usize> Send for ReadHead<T, B> {}
#[allow(unsafe_code)]
unsafe impl<T: Transcendental, const B: usize> Sync for ReadHead<T, B> {}

impl<T: Transcendental, const BUF_SIZE: usize> Default for ReadHead<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> ReadHead<T, BUF_SIZE> {
    /// Create a new `ReadHead` with default delay of 0.5 seconds.
    ///
    /// `resource_name` is the name of the shared tape loop in the buffer registry.
    /// Defaults to `"tape_0"`.
    pub fn new() -> Self {
        Self::with_resource("tape_0")
    }

    /// Create a new `ReadHead` with an explicit resource name.
    pub fn with_resource(resource_name: &str) -> Self {
        let mut metadata = NodeMetadata::new("ReadHead", NodeCategory::Source);
        metadata.parameters = vec![rill_core::ParamMetadata::new(
            "delay",
            rill_core::ParamType::Float,
            ParamValue::Float(0.5),
        )
        .with_range(0.01, 2.0, 0.01)];
        let outputs = vec![Port::output(NodeId(0), 0, "out")];
        Self {
            id: NodeId(0),
            metadata,
            outputs,
            state: NodeState::new(44100.0),
            tape: std::ptr::null(),
            resource_name: resource_name.to_string(),
            delay: 0.5,
            sample_rate: 44100.0,
        }
    }

    /// Set the tape pointer (called during resource resolution).
    pub fn set_tape_ptr(&mut self, tape: *const TapeLoop<T>) {
        self.tape = tape;
    }

    fn delay_samples(&self) -> usize {
        (self.delay * self.sample_rate) as usize
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for ReadHead<T, BUF_SIZE> {
    #[allow(clippy::needless_range_loop)]
    fn generate(
        &mut self,
        _clock: &ClockTick,
        _control_inputs: &[T],
        _clock_inputs: &[ClockTick],
    ) -> ProcessResult<()> {
        debug_assert!(!self.tape.is_null(), "ReadHead: tape not set");
        let tape = unsafe { &*self.tape };
        let delay = self.delay_samples();
        let out = self.outputs[0].buffer.as_mut_array();
        let base = delay + BUF_SIZE - 1;
        for i in 0..BUF_SIZE {
            out[i] = tape.read(base - i);
        }
        self.state.advance();
        Ok(())
    }

    fn num_signal_outputs(&self) -> usize {
        1
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE> for ReadHead<T, BUF_SIZE> {
    fn node_type_id(&self) -> rill_core::NodeTypeId
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
    fn init(&mut self, sr: f32) {
        self.sample_rate = sr;
        self.state.sample_rate = sr;
    }
    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
    }
    fn resolve_resources(&mut self, buffers: &BufferRegistry<T>) {
        if !self.tape.is_null() {
            return;
        }
        if let Some(ptr) = buffers.get_ptr(&self.resource_name) {
            self.tape = ptr as *const TapeLoop<T>;
        }
    }
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "delay" => Some(ParamValue::Float(self.delay)),
            _ => None,
        }
    }
    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if let Some(v) = value.as_f32() {
            match name {
                "delay" => {
                    self.delay = v.clamp(0.01, 2.0);
                    Ok(())
                }
                _ => Err(ProcessError::parameter(format!(
                    "Unknown parameter: {}",
                    name
                ))),
            }
        } else {
            Err(ProcessError::parameter("Expected float value"))
        }
    }
    fn input_port(&self, _: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }
    fn input_port_mut(&mut self, _: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }
    fn output_port(&self, i: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.outputs.get(i)
    }
    fn output_port_mut(&mut self, i: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.outputs.get_mut(i)
    }
    fn control_port(&self, _: usize) -> Option<&Port<T, BUF_SIZE>> {
        None
    }
    fn control_port_mut(&mut self, _: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        None
    }
    fn num_signal_inputs(&self) -> usize {
        0
    }
    fn num_signal_outputs(&self) -> usize {
        1
    }
    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_read_head_creation() {
        let rh = ReadHead::<f32, 64>::new();
        assert!((rh.delay - 0.5).abs() < 1e-6);
        assert_eq!(rh.outputs.len(), 1);
    }
}
