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
    traits::{
        Algorithm, Node, NodeCategory, NodeMetadata, NodeState, ParamMetadata, ParamType,
        ParamValue, Processor,
    },
    NodeId, ParameterId, Port, ProcessError, ProcessResult, RenderContext,
};
use rill_lang::{compile, compile_with, CompileError, RillProgram};

/// Detect whether a rill-lang source string uses graph DSL syntax (contains
/// `param` keyword definitions that create a multi-node graph).
pub fn is_graph_dsl(src: &str) -> bool {
    src.lines().any(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with("param ")
            || trimmed.starts_with("keep param ")
            || trimmed.starts_with("inline param ")
    })
}

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
    registry: Option<std::sync::Arc<rill_lang::builtin::Registry<T>>>,
    sample_rate: f32,
}

/// A graph node driven by a compiled [`rill_lang::graph_engine::RillGraphEngine`].
///
/// Unlike [`LangNode`], which wraps a single [`RillProgram`], this node
/// manages a full graph engine with multiple sub-programs, parameter routing,
/// and a mailbox for inter-thread parameter updates.
pub struct GraphLangNode<T: Transcendental, const BUF_SIZE: usize> {
    id: NodeId,
    metadata: NodeMetadata,
    inputs: Vec<Port<T, BUF_SIZE>>,
    outputs: Vec<Port<T, BUF_SIZE>>,
    controls: Vec<Port<T, BUF_SIZE>>,
    state: NodeState<T, BUF_SIZE>,
    source: String,
    engine: rill_lang::graph_engine::RillGraphEngine<T, BUF_SIZE>,
    sample_rate: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> GraphLangNode<T, BUF_SIZE> {
    /// Build a node from rill-lang source with a built-in registry and sample
    /// rate. Uses [`rill_lang::compile_graph`] which automatically falls back
    /// to single-algorithm mode if the source has no `param` definitions.
    pub fn from_source_with(
        source: &str,
        registry: std::sync::Arc<rill_lang::builtin::Registry<T>>,
        sample_rate: f32,
    ) -> Result<Self, CompileError> {
        let system = rill_core_actor::ActorSystem::new();
        let engine =
            rill_lang::compile_graph::<T, BUF_SIZE>(source, &registry, sample_rate, &system)?;
        let mut metadata = NodeMetadata::new("RillGraphLang", NodeCategory::Processor);
        metadata.type_name = Some("rill/graph_lang".to_string());

        let inputs = vec![Port::input(NodeId(0), 0, "signal_in")];
        let outputs = vec![Port::output(NodeId(0), 0, "signal_out")];

        Ok(Self {
            id: NodeId(0),
            metadata,
            inputs,
            outputs,
            controls: Vec::new(),
            state: NodeState::new(sample_rate),
            source: source.to_string(),
            engine,
            sample_rate,
        })
    }

    /// Build an identity node (`process = _;`) — a safe fallback that always
    /// compiles.
    pub fn identity() -> Self {
        Self::from_source_with(
            "process = _;",
            std::sync::Arc::new(rill_lang::builtin::Registry::new()),
            44_100.0,
        )
        .expect("identity source always compiles")
    }

    /// The rill-lang source backing this node.
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Node<T, BUF_SIZE> for GraphLangNode<T, BUF_SIZE> {
    fn metadata(&self) -> NodeMetadata {
        self.metadata.clone()
    }

    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
        self.sample_rate = sample_rate;
    }

    fn reset(&mut self) {
        self.state.reset();
        Algorithm::reset(&mut self.engine);
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
                let reg = std::sync::Arc::new(rill_lang::builtin::Registry::new());
                let system = rill_core_actor::ActorSystem::new();
                let engine =
                    rill_lang::compile_graph::<T, BUF_SIZE>(&src, &reg, self.sample_rate, &system)
                        .map_err(|e| {
                            ProcessError::parameter(format!("rill-lang compile error: {e}"))
                        })?;
                self.engine = engine;
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

impl<T: Transcendental, const BUF_SIZE: usize> Processor<T, BUF_SIZE>
    for GraphLangNode<T, BUF_SIZE>
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
        self.engine.process(Some(&inp[..]), &mut out[..])?;
        self.state.advance();
        Ok(())
    }
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
            registry: None,
            sample_rate: 44_100.0,
        })
    }

    /// Build a node from rill-lang source with a built-in registry and sample rate.
    pub fn from_source_with(
        source: &str,
        registry: std::sync::Arc<rill_lang::builtin::Registry<T>>,
        sample_rate: f32,
    ) -> Result<Self, CompileError> {
        let program = compile_with::<T>(source, &registry, sample_rate)?;
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
            state: NodeState::new(sample_rate),
            source: source.to_string(),
            program,
            registry: Some(registry),
            sample_rate,
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
        let mut md = self.metadata.clone();
        md.parameters = self
            .program
            .params_meta()
            .iter()
            .map(|p| {
                let mut pm = ParamMetadata::new(
                    &p.name,
                    ParamType::Float,
                    ParamValue::Float(p.default as f32),
                );
                if p.min.is_finite() && p.max.is_finite() {
                    pm = pm.with_range(p.min as f32, p.max as f32, 0.0);
                }
                pm
            })
            .collect();
        md
    }

    fn init(&mut self, sample_rate: f32) {
        self.state.sample_rate = sample_rate;
        self.sample_rate = sample_rate;
    }

    fn reset(&mut self) {
        self.state.reset();
        Algorithm::reset(&mut self.program);
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "source" => Some(ParamValue::String(self.source.clone())),
            _ => self
                .program
                .param_index(id.as_str())
                .map(|idx| ParamValue::Float(self.program.param(idx) as f32)),
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match id.as_str() {
            "source" => {
                let src = match value {
                    ParamValue::String(s) => s,
                    _ => return Err(ProcessError::parameter("`source` must be a string")),
                };
                let program = if let Some(ref reg) = self.registry {
                    compile_with::<T>(&src, reg, self.sample_rate)
                } else {
                    compile::<T>(&src)
                }
                .map_err(|e| ProcessError::parameter(format!("rill-lang compile error: {e}")))?;
                self.program = program;
                self.source = src;
                Ok(())
            }
            _ => {
                if let Some(idx) = self.program.param_index(id.as_str()) {
                    match value {
                        ParamValue::Float(v) => {
                            self.program.set_param(idx, v as f64);
                            Ok(())
                        }
                        _ => Err(ProcessError::parameter("lang param value must be Float")),
                    }
                } else {
                    Err(ProcessError::parameter("unknown parameter"))
                }
            }
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
    fn is_graph_dsl_detects_param_keyword() {
        assert!(is_graph_dsl("param gain: 1.0\nprocess = _ * gain;"));
        assert!(is_graph_dsl("keep param filt_cutoff: 1000.0\nprocess = _;"));
        assert!(is_graph_dsl(
            "inline param offset: 0.0\nprocess = _ + offset;"
        ));
    }

    #[test]
    fn is_graph_dsl_rejects_plain_program() {
        assert!(!is_graph_dsl("process = _ * 0.5;"));
        assert!(!is_graph_dsl("process = lowpass(_, 1000, 0.7);"));
    }

    #[test]
    fn is_graph_dsl_rejects_param_in_middle_of_line() {
        assert!(!is_graph_dsl("process = param(_, 0.5);"));
        assert!(!is_graph_dsl("let param = 4;"));
    }

    #[test]
    fn graph_lang_node_single_algorithm_fallback() {
        let src = "process = _ * 0.5;";
        let reg = std::sync::Arc::new(rill_lang::builtin::Registry::new());
        let mut node = GraphLangNode::<f32, 64>::from_source_with(src, reg, 48_000.0).unwrap();
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
    fn graph_lang_node_identity() {
        let mut node = GraphLangNode::<f32, 64>::identity();
        Node::init(&mut node, 48_000.0);

        {
            let inp = node.input_port_mut(0).unwrap().write();
            inp.fill(3.0);
        }

        let ctx = RenderContext::new(0, 64, 48_000.0);
        node.process(&ctx, &[], &[], &[], &[]).unwrap();

        let out = node.output_port(0).unwrap().read();
        assert_eq!(out[0], 3.0);
    }

    #[test]
    fn graph_lang_node_set_source_recompiles() {
        let mut node = GraphLangNode::<f32, 64>::identity();
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

    // ====================================================================
    // LangNode (single-algorithm) tests
    // ====================================================================

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
