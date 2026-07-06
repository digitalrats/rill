//! Graph-node adapter that wraps a compiled [`rill_lang::RillProgram`] as a
//! signal-graph [`Processor`].
//!
//! This adapter lives in `rill-adrift` (not in `rill-lang`) so that `rill-lang`
//! stays free of a `rill-graph`/`rill-core` node dependency. The node exposes a
//! single signal input and a single signal output and delegates block
//! processing to the compiled program. The rill-lang source is carried as a
//! `source` string parameter, so a serialized graph can embed a rill-lang block
//! and recompile it on load (or on a control-thread `set_parameter`).

use rill_core::{
    math::Transcendental,
    traits::{Algorithm, Node, NodeCategory, NodeMetadata, NodeState, Processor},
    NodeId, ParamValue, ParameterId, Port, ProcessError, ProcessResult, RenderContext,
};
use rill_lang::{compile, CompileError, RillProgram};

/// A graph node whose per-block math is defined by rill-lang source.
pub struct LangNode<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
    controls: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    source: String,
    program: RillProgram<T>,
}

impl<T: Transcendental, const BUF_SIZE: usize> LangNode<T, BUF_SIZE> {
    /// Build a node from rill-lang source. Returns a [`CompileError`] if the
    /// source fails to compile.
    pub fn from_source(source: &str) -> Result<Self, CompileError> {
        let program = compile::<T>(source)?;
        let mut metadata = NodeMetadata::new("RillLang", NodeCategory::Processor);
        metadata.type_name = Some("rill/lang".to_string());

        let inputs = vec![Port::input(NodeId(0), 0, "signal_in")];
        let outputs = vec![Port::output(NodeId(0), 0, "signal_out")];

        Ok(Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(44_100.0),
            source: source.to_string(),
            program,
        })
    }

    /// Build an identity node (`process = _;`) — a safe fallback that always
    /// compiles.
    pub fn identity() -> Self {
        Self::from_source("process = _;").expect("identity source always compiles")
    }

    /// The rill-lang source backing this node.
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for LangNode<T, BUF_SIZE> {
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }

    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
    }

    fn reset(&mut self) {
        self.state.reset();
        Algorithm::reset(&mut self.program);
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "source" => Some(ParamValue::String(self.source.clone())),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match id.as_str() {
            "source" => {
                let src = match value {
                    ParamValue::String(s) => s,
                    _ => return Err(ProcessError::parameter("`source` must be a string")),
                };
                let program = compile::<T>(&src).map_err(|e| {
                    ProcessError::parameter(format!("rill-lang compile error: {e}"))
                })?;
                self.program = program;
                self.source = src;
                Ok(())
            }
            _ => Err(ProcessError::parameter("unknown parameter")),
        }
    }

    fn id(&self) -> NodeId {
        self.id
    }

    fn set_id(&mut self, id: NodeId) {
        self.id = id;
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

    fn state(&self) -> &NodeState<T, BUF_SIZE> {
        &self.state
    }

    fn state_mut(&mut self) -> &mut NodeState<T, BUF_SIZE> {
        &mut self.state
    }

    fn num_signal_inputs(&self) -> usize {
        self.inputs.len()
    }

    fn num_signal_outputs(&self) -> usize {
        self.outputs.len()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE> for LangNode<T, BUF_SIZE> {
    fn process(
        &mut self,
        _ctx: &RenderContext,
        _signal_inputs: &[&[T; BUF_SIZE]],
        _control_inputs: &[T],
        _clock_inputs: &[RenderContext],
        _feedback_inputs: &[&[T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        // Disjoint field borrows: `inputs`, `outputs`, `program`.
        let inp = self.inputs[0].read();
        let out = self.outputs[0].write();
        self.program.process(Some(&inp[..]), &mut out[..])?;
        self.state.advance();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_source_rejects_bad_program() {
        assert!(LangNode::<f32, 64>::from_source("process = _ , _;").is_err());
    }

    #[test]
    fn node_halves_input_block() {
        let mut node = LangNode::<f32, 64>::from_source("process = _ * 0.5;").unwrap();
        Node::init(&mut node, 48_000.0);

        {
            let inp = node.input_port_mut(0).unwrap().write();
            for (i, v) in inp.iter_mut().enumerate() {
                *v = i as f32;
            }
        }

        let ctx = RenderContext::new(0, 64, 48_000.0);
        node.process(&ctx, &[], &[], &[], &[]).unwrap();

        let out = node.output_port(0).unwrap().read();
        for (i, sample) in out.iter().enumerate() {
            assert_eq!(*sample, i as f32 * 0.5);
        }
    }

    #[test]
    fn set_source_recompiles() {
        let mut node = LangNode::<f32, 64>::identity();
        node.set_parameter(
            &ParameterId::new("source").unwrap(),
            ParamValue::String("process = _ * 2;".to_string()),
        )
        .unwrap();
        assert_eq!(node.source(), "process = _ * 2;");

        {
            let inp = node.input_port_mut(0).unwrap().write();
            inp.fill(3.0);
        }
        let ctx = RenderContext::new(0, 64, 48_000.0);
        node.process(&ctx, &[], &[], &[], &[]).unwrap();
        let out = node.output_port(0).unwrap().read();
        assert_eq!(out[0], 6.0);
    }

    #[test]
    fn set_source_rejects_bad_program() {
        let mut node = LangNode::<f32, 64>::identity();
        assert!(node
            .set_parameter(
                &ParameterId::new("source").unwrap(),
                ParamValue::String("process = _ , _;".to_string()),
            )
            .is_err());
    }
}
