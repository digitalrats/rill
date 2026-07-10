use rill_core::{
    buffer::{ResourceRegistry, TapeWriter},
    math::vector::scalar::ScalarVector4,
    math::vector::traits::Vector as VecTrait,
    math::Transcendental,
    ParamValue, ParameterId, ProcessError, ProcessResult, RenderContext,
};

// Holds an `Rc`-based tape handle — the graph is single-threaded and moved
// to the signal thread once; `Send`/`Sync` are asserted for that pattern.
#[allow(unsafe_code)]
unsafe impl<T: Transcendental, const B: usize> Send for WriteHead<T, B> {}
#[allow(unsafe_code)]
unsafe impl<T: Transcendental, const B: usize> Sync for WriteHead<T, B> {}

/// Write head node — mixes input signal with feedback and writes into a
/// shared [`TapeLoop`] that [`ReadHead`](crate::read_head::ReadHead) nodes
/// read from.
///
/// The tape loop is allocated by the graph's resource registry during
/// node initialization.
///
/// # Signal ports
/// - 1 audio input (dry)
/// - 1 feedback input (from feedback loop)
/// - 1 main output (forward path passthrough)
///
/// # Parameters
/// - `delay_time` (0.01 – 2.0 s)
/// - `feedback`   (0.0 – 0.99)
pub struct WriteHead<T: Transcendental, const BUF_SIZE: usize> {
    // (removed legacy field)
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
    // (removed legacy field)

    tape: Option<TapeWriter<T>>,
    resource_name: String,
    delay_time: f32,
    feedback: f32,
    sample_rate: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> WriteHead<T, BUF_SIZE> {
    /// Create a new `WriteHead` with default delay (0.5 s) and feedback (0.3).
    ///
    /// `resource_name` is the name of the shared tape loop in the buffer registry.
    /// Defaults to `"tape_0"`.
    pub fn new(sample_rate: f32) -> Self {
        Self::with_resource(sample_rate, "tape_0")
    }

    /// Create a new `WriteHead` with an explicit resource name.
    pub fn with_resource(sample_rate: f32, resource_name: &str) -> Self {
        let mut metadata = NodeMetadata::new("WriteHead", NodeCategory::Processor);
        metadata.parameters = vec![
            rill_core::ParamMetadata::new(
                "delay_time",
                rill_core::ParamType::Float,
                ParamValue::Float(0.5),
            )
            .with_range(0.01, 2.0, 0.01),
            rill_core::ParamMetadata::new(
                "feedback",
                rill_core::ParamType::Float,
                ParamValue::Float(0.3),
            )
            .with_range(0.0, 0.99, 0.01),
        ];

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        inputs.push(Port::input(NodeId(0), 0, "signal_in"));
        inputs.push(Port::input(NodeId(0), 1, "feedback_in"));
        outputs.push(Port::output(NodeId(0), 0, "main_out"));

        Self {
    // (removed legacy field)
            metadata,
            inputs,
            outputs,
    // (removed legacy field)
            tape: None,
            resource_name: resource_name.to_string(),
            delay_time: 0.5,
            feedback: 0.3,
            sample_rate,
        }
    }
}

// ── Processor trait ──────────────────────────────────────────────────────

        &mut self,
        _ctx: &RenderContext,
        _signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        let Some(tape) = self.tape.as_mut() else {
            debug_assert!(false, "WriteHead: tape not set");
            return Ok(());
        };

        let input_buf = self.inputs[0].read();
        let fb_gain = T::from_f32(self.feedback);
        let zero_buf = [T::ZERO; BUF_SIZE];
        // Feedback arrives on the `feedback_in` port's delayed feedback buffer
        // (filled by the upstream node's `snapshot_feedback`), not via the
        // `feedback_inputs` argument (which the engine leaves empty — nodes read
        // their own port buffers). Reading `feedback_buffer` directly gives the
        // fresh 1-block-delayed value without the accumulation that `buffer`
        // (via `pre_process`) would incur on a feedback-only input.
        let fb_buf = self.inputs[1].feedback().unwrap_or(&zero_buf);
        let chunks = BUF_SIZE / 4;
        let fg = ScalarVector4::splat(fb_gain);

        for chunk in 0..chunks {
            let o = chunk * 4;
            let i_v = ScalarVector4::load(&input_buf[o..o + 4]);
            let f_v = ScalarVector4::load(&fb_buf[o..o + 4]);
            let w = i_v.add(&f_v.mul(&fg));

            for k in 0..4 {
                tape.write(w.extract(k));
            }
        }

        // Remainder
        for i in chunks * 4..BUF_SIZE {
            tape.write(input_buf[i] + fb_buf[i] * fb_gain);
        }

        self.outputs[0].write_from(input_buf);
        self.state.advance();
        Ok(())
    }

    fn latency(&self) -> usize {
        0
    }

// ── Node trait ─────────────────────────────────────────────────────

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
        self.metadata.clone()
    }
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.state.sample_rate = sample_rate;
    }
    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
    }

    fn resolve_resources(&mut self, resources: &mut ResourceRegistry<T>) {
        if self.tape.is_some() {
            return;
        }
        self.tape = resources.writer(&self.resource_name);
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "delay_time" => Some(ParamValue::Float(self.delay_time)),
            "feedback" => Some(ParamValue::Float(self.feedback)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        let name = id.as_str();
        if let Some(v) = value.as_f32() {
            match name {
                "delay_time" => {
                    self.delay_time = v.clamp(0.01, 2.0);
                    Ok(())
                }
                "feedback" => {
                    self.feedback = v.clamp(0.0, 0.99);
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

    fn input_port(&self, i: usize) -> Option<&Port<T, BUF_SIZE>> {
        self.inputs.get(i)
    }
    fn input_port_mut(&mut self, i: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        self.inputs.get_mut(i)
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
        2
    }
    fn num_signal_outputs(&self) -> usize {
        1
    }
    fn num_feedback_ports(&self) -> usize {
        0
    }
        &self.state
    }
        &mut self.state
    }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_head_creation() {
        let wh = WriteHead::<f32, 64>::new(44100.0);
        assert!((wh.delay_time - 0.5).abs() < 1e-6);
        assert!((wh.feedback - 0.3).abs() < 1e-6);
        assert_eq!(wh.inputs.len(), 2);
        assert_eq!(wh.outputs.len(), 1);
    }

    #[test]
    fn test_write_head_params() {
        let mut wh = WriteHead::<f32, 64>::new(44100.0);
        wh.set_parameter(
            &ParameterId::new("feedback").unwrap(),
            ParamValue::Float(0.5),
        )
        .unwrap();
        assert!((wh.feedback - 0.5).abs() < 1e-6);
    }
}
