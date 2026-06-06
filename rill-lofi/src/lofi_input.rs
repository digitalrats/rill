use rill_core::{
    io::IoBackend,
    math::Transcendental,
    traits::{IoNode, Node, Source},
    NodeCategory, NodeId, NodeMetadata, NodeState, ParamMetadata, ParamType, ParamValue,
    ParameterId, Port, ProcessResult, RenderContext,
};

use crate::config::LofiConfig;
use crate::lofi_processor::LofiProcessor;

/// Source node wrapping an [`IoBackend<T>`] with lofi processing.
///
/// Follows `Input<T, BUF_SIZE>` pattern from rill-io. Reads audio from backend,
/// applies lofi processing (bitcrush, noise, DAC, delay), fills output ports.
/// Chip control goes through `write_to_backend(data)` or `set_parameter("io_write", ...)`.
pub struct LofiInput<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    outputs: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    backend: Option<Box<dyn IoBackend<T>>>,
    bufs: Vec<[T; BUF_SIZE]>,
    lofi: LofiProcessor<BUF_SIZE>,
}

impl<T: Transcendental, const BUF_SIZE: usize> LofiInput<T, BUF_SIZE> {
    /// Create a mono LofiInput with the given lofi configuration.
    pub fn new(lofi_config: LofiConfig) -> Self {
        Self::with_channels(1, lofi_config)
    }

    /// Create a LofiInput with `num` output channels.
    pub fn with_channels(num: usize, lofi_config: LofiConfig) -> Self {
        let metadata = NodeMetadata {
            name: "Lofi Input".to_string(),
            type_name: None,
            category: NodeCategory::Source,
            description: "Lo-fi processed input source".to_string(),
            author: "Rill Lo-Fi".to_string(),
            version: "0.3.0".to_string(),
            signal_inputs: 0,
            signal_outputs: num,
            control_inputs: 0,
            control_outputs: 0,
            clock_inputs: 0,
            clock_outputs: 0,
            feedback_ports: 0,
            parameters: vec![
                ParamMetadata::new("enable_bitcrush", ParamType::Bool, ParamValue::Bool(true))
                    .with_description("Enable bitcrushing"),
                ParamMetadata::new("enable_noise", ParamType::Bool, ParamValue::Bool(true))
                    .with_description("Enable vintage noise"),
                ParamMetadata::new("dry_wet", ParamType::Float, ParamValue::Float(1.0))
                    .with_description("Dry/wet mix")
                    .with_range(0.0, 1.0, 0.01),
                ParamMetadata::new("output_gain", ParamType::Float, ParamValue::Float(1.0))
                    .with_description("Output gain")
                    .with_range(0.0, 4.0, 0.1),
            ],
        };
        let name = move |i: usize| -> String {
            if num == 1 {
                "out".into()
            } else {
                format!("ch_{i}")
            }
        };
        let outputs = (0..num)
            .map(|i| Port::output(NodeId(0), i as u16, &name(i)))
            .collect();
        let bufs = vec![[T::ZERO; BUF_SIZE]; num];

        Self {
            id: NodeId(0),
            metadata,
            outputs,
            state: NodeState::new(44100.0),
            backend: None,
            bufs,
            lofi: LofiProcessor::new(lofi_config),
        }
    }

    /// Returns `true` if a backend is attached.
    pub fn has_backend(&self) -> bool {
        self.backend.is_some()
    }

    /// Forward raw bytes to the backend via [`IoControl::write_data`](rill_core::io::IoControl::write_data).
    pub fn write_to_backend(&self, data: &[u8]) -> usize {
        self.backend
            .as_ref()
            .and_then(|b| b.as_control())
            .map(|c| c.write_data(data))
            .unwrap_or(0)
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for LofiInput<T, BUF_SIZE> {
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }
    fn node_type_id(&self) -> rill_core::NodeTypeId {
        rill_core::NodeTypeId::of::<Self>()
    }
    fn id(&self) -> NodeId {
        self.id
    }
    fn set_id(&mut self, id: NodeId) {
        self.id = id;
    }
    fn init(&mut self, sample_rate: f32) {
        self.state = NodeState::new(sample_rate);
        self.lofi.init(sample_rate);
    }
    fn reset(&mut self) {
        self.state.reset();
        self.lofi.reset();
    }
    fn as_io_node_mut(&mut self) -> Option<&mut dyn IoNode<T, BUF_SIZE>> {
        Some(self)
    }
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        self.lofi.get_parameter(id)
    }
    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        if id.as_str() == "io_write" {
            if let Some(bytes) = value.as_bytes() {
                self.write_to_backend(bytes);
                return Ok(());
            }
            return Err(rill_core::ProcessError::parameter("io_write expects Bytes"));
        }
        self.lofi.set_parameter(id, value)
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
        self.outputs.len()
    }
    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }
    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> IoNode<T, BUF_SIZE> for LofiInput<T, BUF_SIZE> {
    fn resolve_backend(&mut self, backend: Box<dyn IoBackend<T>>) {
        self.backend = Some(backend);
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Source<T, BUF_SIZE> for LofiInput<T, BUF_SIZE> {
    fn generate(
        &mut self,
        _ctx: &RenderContext,
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
    ) -> ProcessResult<()> {
        if let Some(ref io) = self.backend {
            let nch = self.outputs.len();
            if nch == 0 {
                self.state.advance();
                return Ok(());
            }
            let mut channels: Vec<&mut [T]> = self.bufs.iter_mut().map(|b| &mut b[..]).collect();
            let n = io.read(&mut channels);
            for buf in self.bufs.iter_mut() {
                for s in buf[..n.min(BUF_SIZE)].iter_mut() {
                    *s = T::from_f32(self.lofi.process_sample(s.to_f32()));
                }
            }
            if n >= BUF_SIZE {
                for (i, buf) in self.bufs.iter().enumerate() {
                    if let Some(port) = self.outputs.get_mut(i) {
                        port.buffer_mut().as_mut_array()[..BUF_SIZE]
                            .copy_from_slice(&buf[..BUF_SIZE]);
                    }
                }
            }
        }
        self.state.advance();
        Ok(())
    }
}
