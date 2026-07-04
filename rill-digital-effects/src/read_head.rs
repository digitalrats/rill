use rill_core::{
    buffer::{BufferRegistry, TapeLoop},
    math::Transcendental,
    traits::{Node, NodeCategory, NodeMetadata, NodeState, Source},
    ClockTick, NodeId, ParamValue, ParameterId, Port, ProcessError, ProcessResult, RenderContext,
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
    /// Smoothed read position in fractional samples. Glides toward the target
    /// (`delay * sample_rate`) to avoid zipper noise when `delay` is modulated,
    /// which also produces the pitch glide of tape wow/flutter.
    current_delay_samples: f64,
    /// Per-sample one-pole glide coefficient toward the target delay.
    delay_smoothing: f64,
}

/// Time constant of the delay glide, in seconds.
const DELAY_SMOOTH_SECONDS: f64 = 0.008;

/// Per-sample one-pole coefficient that reaches ~63% of a step in
/// [`DELAY_SMOOTH_SECONDS`].
fn delay_smoothing_coeff(sample_rate: f64) -> f64 {
    1.0 - (-1.0 / (DELAY_SMOOTH_SECONDS * sample_rate)).exp()
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
            current_delay_samples: 0.5 * 44100.0,
            delay_smoothing: delay_smoothing_coeff(44100.0),
        }
    }

    /// Set the tape pointer (called during resource resolution).
    pub fn set_tape_ptr(&mut self, tape: *const TapeLoop<T>) {
        self.tape = tape;
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for ReadHead<T, BUF_SIZE> {
    #[allow(clippy::needless_range_loop)]
    fn generate(
        &mut self,
        _ctx: &RenderContext,
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _tick: &ClockTick,
    ) -> ProcessResult<()> {
        debug_assert!(!self.tape.is_null(), "ReadHead: tape not set");
        let tape = unsafe { &*self.tape };
        let target = (self.delay as f64) * (self.sample_rate as f64);
        let glide = self.delay_smoothing;
        let mut current = self.current_delay_samples;
        let out = self.outputs[0].buffer.as_mut_array();
        let n = BUF_SIZE;
        for i in 0..n {
            // Earlier samples in the block sit further back on the tape.
            let d = current + (n - 1 - i) as f64;
            out[i] = tape.read_interpolated(d.max(0.0));
            // Glide the read position toward the target for the next (newer) sample.
            current += (target - current) * glide;
        }
        self.current_delay_samples = current;
        self.state.advance();
        Ok(())
    }

    fn num_signal_outputs(&self) -> usize {
        1
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for ReadHead<T, BUF_SIZE> {
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
        self.current_delay_samples = (self.delay as f64) * (sr as f64);
        self.delay_smoothing = delay_smoothing_coeff(sr as f64);
    }
    fn reset(&mut self) {
        self.state.sample_pos = 0;
        self.state.blocks_processed = 0;
        self.current_delay_samples = (self.delay as f64) * (self.sample_rate as f64);
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

    /// Build a tape pre-filled with a rising ramp `0.0, 1.0, .. (n-1)`.
    /// After writing, `read(0)` returns `n-1`, `read(d)` returns `n-1-d`.
    fn ramp_tape(n: usize) -> TapeLoop<f32> {
        let mut tape = TapeLoop::<f32>::new(1024).unwrap();
        for i in 0..n {
            tape.write(i as f32);
        }
        tape
    }

    fn delay_param(v: f32) -> (ParameterId, ParamValue) {
        (ParameterId::new("delay").unwrap(), ParamValue::Float(v))
    }

    #[test]
    fn test_read_head_creation() {
        let rh = ReadHead::<f32, 64>::new();
        assert!((rh.delay - 0.5).abs() < 1e-6);
        assert_eq!(rh.outputs.len(), 1);
    }

    /// Regression: an integer sample delay must read exact tape samples.
    /// sr=100, delay=0.1s → 10 samples. base = 10 + 4 - 1 = 13.
    /// out[i] = read(13 - i) = [26, 27, 28, 29].
    #[test]
    fn read_head_integer_delay_reads_exact_samples() {
        let tape = ramp_tape(40);
        let mut rh = ReadHead::<f32, 4>::new();
        let (id, v) = delay_param(0.1);
        rh.set_parameter(&id, v).unwrap();
        rh.init(100.0);
        rh.set_tape_ptr(&tape as *const _);

        let ctx = RenderContext::new(0, 4, 100.0);
        let tick = ClockTick::new(0, 4, 100.0, String::new());
        rh.generate(&ctx, &[], &[], &tick).unwrap();

        let out = rh.outputs[0].buffer.as_array();
        assert_eq!(out[0], 26.0);
        assert_eq!(out[3], 29.0);
    }

    /// A fractional sample delay must be linearly interpolated, not truncated.
    /// sr=100, delay=0.105s → 10.5 samples. out[0] = read_interp(13.5),
    /// between read(13)=26 and read(14)=25 → 25.5.
    #[test]
    fn read_head_fractional_delay_interpolates() {
        let tape = ramp_tape(40);
        let mut rh = ReadHead::<f32, 4>::new();
        let (id, v) = delay_param(0.105);
        rh.set_parameter(&id, v).unwrap();
        rh.init(100.0);
        rh.set_tape_ptr(&tape as *const _);

        let ctx = RenderContext::new(0, 4, 100.0);
        let tick = ClockTick::new(0, 4, 100.0, String::new());
        rh.generate(&ctx, &[], &[], &tick).unwrap();

        let out = rh.outputs[0].buffer.as_array();
        assert!(
            out[0] > 25.0 && out[0] < 26.0,
            "expected interpolated ~25.5, got {}",
            out[0]
        );
        assert!((out[0] - 25.5).abs() < 0.01, "got {}", out[0]);
    }

    /// An abrupt delay change must glide, not jump instantly. After snapping the
    /// current delay to 10 samples (delay=0.1) we request 30 samples (delay=0.3).
    /// The first output must still reflect the old delay (~10 → ~26), not an
    /// instant jump to 30 samples (~6).
    #[test]
    fn read_head_delay_change_glides_not_jumps() {
        let tape = ramp_tape(40);
        let mut rh = ReadHead::<f32, 4>::new();
        let (id, v) = delay_param(0.1);
        rh.set_parameter(&id, v).unwrap();
        rh.init(100.0);
        rh.set_tape_ptr(&tape as *const _);

        let (id2, v2) = delay_param(0.3);
        rh.set_parameter(&id2, v2).unwrap();

        let ctx = RenderContext::new(0, 4, 100.0);
        let tick = ClockTick::new(0, 4, 100.0, String::new());
        rh.generate(&ctx, &[], &[], &tick).unwrap();

        let out = rh.outputs[0].buffer.as_array();
        assert!(
            out[0] > 20.0,
            "delay jumped instead of gliding: out[0]={}",
            out[0]
        );
    }
}
