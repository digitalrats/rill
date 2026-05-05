//! Processable trait and NodeVariant.

use crate::math::Transcendental;
use crate::queues::telemetry::TelemetryTx;
use crate::time::ClockTick;
use crate::traits::port::Port;
use crate::traits::ProcessResult;
use crate::traits::SignalNode;

// ============================================================================
// ProcessContext
// ============================================================================

/// Processing context provided during signal graph execution.
pub struct ProcessContext<'a> {
    /// Current clock tick with sample-accurate timing information.
    pub clock: &'a ClockTick,
}

// ============================================================================
// Processable Trait
// ============================================================================

/// A node or component that can process a block of audio.
///
/// This is the main execution trait for the signal graph. Each node type
/// (Source, Processor, Router, Sink) implements this via blanket impls
/// that delegate to their respective `generate`/`process`/`route`/`consume`.
pub trait Processable<T: Transcendental, const BUF_SIZE: usize> {
    /// Process one block of audio samples.
    ///
    /// # Errors
    /// Returns a [`ProcessError`](crate::traits::ProcessError) if processing fails.
    fn process_block(&mut self, ctx: &mut ProcessContext) -> ProcessResult<()>;
}

// ============================================================================
// Blanket Implementations
// ============================================================================

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Source<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &mut ProcessContext) -> ProcessResult<()> {
        self.as_mut().generate(ctx.clock, &[], &[])
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Processor<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &mut ProcessContext) -> ProcessResult<()> {
        self.as_mut().process(ctx.clock, &[], &[], &[], &[])
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Sink<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &mut ProcessContext) -> ProcessResult<()> {
        self.as_mut().consume(ctx.clock, &[], &[], &[], &[])
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Router<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &mut ProcessContext) -> ProcessResult<()> {
        (**self).route(ctx.clock, &[])
    }
}

// ============================================================================
// NodeVariant
// ============================================================================

/// A type-erased node that wraps any of the four node roles.
///
/// Dispatches `process_block` and `SignalNode` methods to the inner node.
pub enum NodeVariant<T: Transcendental, const BUF_SIZE: usize> {
    /// Signal source node (generates audio).
    Source(Box<dyn crate::traits::Source<T, BUF_SIZE>>),
    /// Signal processor node (processes audio in-place).
    Processor(Box<dyn crate::traits::Processor<T, BUF_SIZE>>),
    /// Signal router node (routes signals between ports).
    Router(Box<dyn crate::traits::Router<T, BUF_SIZE>>),
    /// Signal sink node (consumes audio).
    Sink(Box<dyn crate::traits::Sink<T, BUF_SIZE>>),
}

impl<T: Transcendental, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for NodeVariant<T, BUF_SIZE>
{
    fn process_block(&mut self, ctx: &mut ProcessContext) -> ProcessResult<()> {
        match self {
            NodeVariant::Source(src) => src.process_block(ctx),
            NodeVariant::Processor(proc) => proc.process_block(ctx),
            NodeVariant::Router(rt) => rt.process_block(ctx),
            NodeVariant::Sink(sink) => sink.process_block(ctx),
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> NodeVariant<T, BUF_SIZE> {
    /// Set the telemetry sender for the wrapped node.
    pub fn set_telemetry_tx(&mut self, tx: TelemetryTx) {
        match self {
            NodeVariant::Source(src) => SignalNode::set_telemetry_tx(src.as_mut(), tx),
            NodeVariant::Processor(proc) => SignalNode::set_telemetry_tx(proc.as_mut(), tx),
            NodeVariant::Router(rt) => SignalNode::set_telemetry_tx(rt.as_mut(), tx),
            NodeVariant::Sink(sink) => SignalNode::set_telemetry_tx(sink.as_mut(), tx),
        }
    }
}

// ============================================================================
// SignalNode for NodeVariant
// ============================================================================

impl<T: Transcendental, const BUF_SIZE: usize> SignalNode<T, BUF_SIZE>
    for NodeVariant<T, BUF_SIZE>
{
    fn node_type_id(&self) -> crate::traits::NodeTypeId
    where
        Self: 'static + Sized,
    {
        unreachable!()
    }
    fn id(&self) -> crate::traits::NodeId {
        match self {
            NodeVariant::Source(src) => src.id(),
            NodeVariant::Processor(proc) => proc.id(),
            NodeVariant::Router(rt) => rt.id(),
            NodeVariant::Sink(sink) => sink.id(),
        }
    }
    fn set_id(&mut self, id: crate::traits::NodeId) {
        match self {
            NodeVariant::Source(src) => src.set_id(id),
            NodeVariant::Processor(proc) => proc.set_id(id),
            NodeVariant::Router(rt) => rt.set_id(id),
            NodeVariant::Sink(sink) => sink.set_id(id),
        }
    }
    fn metadata(&self) -> crate::traits::NodeMetadata {
        match self {
            NodeVariant::Source(src) => src.metadata(),
            NodeVariant::Processor(proc) => proc.metadata(),
            NodeVariant::Router(rt) => rt.metadata(),
            NodeVariant::Sink(sink) => sink.metadata(),
        }
    }
    fn init(&mut self, sample_rate: f32) {
        match self {
            NodeVariant::Source(src) => src.init(sample_rate),
            NodeVariant::Processor(proc) => proc.init(sample_rate),
            NodeVariant::Router(rt) => rt.init(sample_rate),
            NodeVariant::Sink(sink) => sink.init(sample_rate),
        }
    }
    fn resolve_resources(&mut self, buffers: &crate::buffer::BufferRegistry<T>) {
        match self {
            NodeVariant::Source(src) => src.resolve_resources(buffers),
            NodeVariant::Processor(proc) => proc.resolve_resources(buffers),
            NodeVariant::Router(rt) => rt.resolve_resources(buffers),
            NodeVariant::Sink(sink) => sink.resolve_resources(buffers),
        }
    }
    fn resolve_backend(&mut self, backend: *mut dyn crate::io::IoBackend<T>) {
        match self {
            NodeVariant::Source(src) => src.resolve_backend(backend),
            NodeVariant::Processor(proc) => proc.resolve_backend(backend),
            NodeVariant::Router(rt) => rt.resolve_backend(backend),
            NodeVariant::Sink(sink) => sink.resolve_backend(backend),
        }
    }
    fn reset(&mut self) {
        match self {
            NodeVariant::Source(src) => src.reset(),
            NodeVariant::Processor(proc) => proc.reset(),
            NodeVariant::Router(rt) => rt.reset(),
            NodeVariant::Sink(sink) => sink.reset(),
        }
    }
    fn get_parameter(&self, id: &crate::traits::ParameterId) -> Option<crate::traits::ParamValue> {
        match self {
            NodeVariant::Source(src) => src.get_parameter(id),
            NodeVariant::Processor(proc) => proc.get_parameter(id),
            NodeVariant::Router(rt) => rt.get_parameter(id),
            NodeVariant::Sink(sink) => sink.get_parameter(id),
        }
    }
    fn set_parameter(
        &mut self,
        id: &crate::traits::ParameterId,
        value: crate::traits::ParamValue,
    ) -> ProcessResult<()> {
        match self {
            NodeVariant::Source(src) => src.set_parameter(id, value),
            NodeVariant::Processor(proc) => proc.set_parameter(id, value),
            NodeVariant::Router(rt) => rt.set_parameter(id, value),
            NodeVariant::Sink(sink) => sink.set_parameter(id, value),
        }
    }
    fn input_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.input_port(index),
            NodeVariant::Processor(proc) => proc.input_port(index),
            NodeVariant::Router(rt) => rt.input_port(index),
            NodeVariant::Sink(sink) => sink.input_port(index),
        }
    }
    fn input_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.input_port_mut(index),
            NodeVariant::Processor(proc) => proc.input_port_mut(index),
            NodeVariant::Router(rt) => rt.input_port_mut(index),
            NodeVariant::Sink(sink) => sink.input_port_mut(index),
        }
    }
    fn output_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.output_port(index),
            NodeVariant::Processor(proc) => proc.output_port(index),
            NodeVariant::Router(rt) => rt.output_port(index),
            NodeVariant::Sink(sink) => sink.output_port(index),
        }
    }
    fn output_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.output_port_mut(index),
            NodeVariant::Processor(proc) => proc.output_port_mut(index),
            NodeVariant::Router(rt) => rt.output_port_mut(index),
            NodeVariant::Sink(sink) => sink.output_port_mut(index),
        }
    }
    fn control_port(&self, index: usize) -> Option<&Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.control_port(index),
            NodeVariant::Processor(proc) => proc.control_port(index),
            NodeVariant::Router(rt) => rt.control_port(index),
            NodeVariant::Sink(sink) => sink.control_port(index),
        }
    }
    fn control_port_mut(&mut self, index: usize) -> Option<&mut Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.control_port_mut(index),
            NodeVariant::Processor(proc) => proc.control_port_mut(index),
            NodeVariant::Router(rt) => rt.control_port_mut(index),
            NodeVariant::Sink(sink) => sink.control_port_mut(index),
        }
    }
    fn num_signal_inputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**src;
                n.num_signal_inputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**proc;
                n.num_signal_inputs()
            }
            NodeVariant::Router(rt) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**rt;
                n.num_signal_inputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**sink;
                n.num_signal_inputs()
            }
        }
    }
    fn num_signal_outputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**src;
                n.num_signal_outputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**proc;
                n.num_signal_outputs()
            }
            NodeVariant::Router(rt) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**rt;
                n.num_signal_outputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**sink;
                n.num_signal_outputs()
            }
        }
    }
    fn num_control_inputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**src;
                n.num_control_inputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**proc;
                n.num_control_inputs()
            }
            NodeVariant::Router(rt) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**rt;
                n.num_control_inputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**sink;
                n.num_control_inputs()
            }
        }
    }
    fn num_control_outputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**src;
                n.num_control_outputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**proc;
                n.num_control_outputs()
            }
            NodeVariant::Router(rt) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**rt;
                n.num_control_outputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**sink;
                n.num_control_outputs()
            }
        }
    }
    fn num_clock_inputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**src;
                n.num_clock_inputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**proc;
                n.num_clock_inputs()
            }
            NodeVariant::Router(rt) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**rt;
                n.num_clock_inputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**sink;
                n.num_clock_inputs()
            }
        }
    }
    fn num_clock_outputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**src;
                n.num_clock_outputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**proc;
                n.num_clock_outputs()
            }
            NodeVariant::Router(rt) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**rt;
                n.num_clock_outputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**sink;
                n.num_clock_outputs()
            }
        }
    }
    fn num_feedback_ports(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**src;
                n.num_feedback_ports()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**proc;
                n.num_feedback_ports()
            }
            NodeVariant::Router(rt) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**rt;
                n.num_feedback_ports()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn SignalNode<T, BUF_SIZE> = &**sink;
                n.num_feedback_ports()
            }
        }
    }
    fn state(&self) -> &crate::traits::NodeState<T, BUF_SIZE> {
        match self {
            NodeVariant::Source(src) => src.state(),
            NodeVariant::Processor(proc) => proc.state(),
            NodeVariant::Router(rt) => rt.state(),
            NodeVariant::Sink(sink) => sink.state(),
        }
    }
    fn state_mut(&mut self) -> &mut crate::traits::NodeState<T, BUF_SIZE> {
        match self {
            NodeVariant::Source(src) => src.state_mut(),
            NodeVariant::Processor(proc) => proc.state_mut(),
            NodeVariant::Router(rt) => rt.state_mut(),
            NodeVariant::Sink(sink) => sink.state_mut(),
        }
    }
}
