//! Processable trait for unified audio node processing.
//!
//! This module defines the `Processable` trait that unifies `generate`, `process`, and `consume`
//! into a single `process_block` method, making it easier to build generic signal graphs.

use crate::math::Transcendental;
use crate::queues::telemetry::{Telemetry, TelemetryTx, CLOCK_TICK};
use crate::time::ClockTick;
use crate::traits::ProcessResult;
use crate::traits::SignalNode;

// ============================================================================
// ProcessContext
// ============================================================================

/// Convenience structure that gathers all input buffers for a node.
///
/// Nodes write their output directly into their own output port buffers
/// (accessible via `SignalNode::output_port_mut`), so only input data
/// is passed through this context.
pub struct ProcessContext<'a, T: Transcendental, const BUF_SIZE: usize> {
    /// Current clock tick
    pub clock: &'a ClockTick,
    /// Audio input buffers (slice of references to [T; BUF_SIZE])
    pub signal_inputs: &'a [&'a [T; BUF_SIZE]],
    /// Control input values (slice of T)
    pub control_inputs: &'a [T],
    /// Clock input ticks
    pub clock_inputs: &'a [ClockTick],
    /// Feedback input buffers (slice of references to [T; BUF_SIZE])
    pub feedback_inputs: &'a [&'a [T; BUF_SIZE]],
    /// Optional telemetry sender available to all nodes.
    /// Nodes can push events (clock tick, peaks, status) from their
    /// `generate()` / `process()` / `consume()` methods without
    /// storing a sender locally.
    pub telemetry_tx: Option<&'a TelemetryTx>,
}

// ============================================================================
// Processable Trait
// ============================================================================

/// Unified trait for processing audio nodes.
///
/// This trait is implemented for all `Source`, `Processor`, and `Sink` types,
/// providing a single method that dispatches to the appropriate underlying
/// method (`generate`, `process`, or `consume`).
pub trait Processable<T: Transcendental, const BUF_SIZE: usize> {
    /// Process a single block of audio.
    ///
    /// The default implementation uses the node's category to call the
    /// appropriate subtrait method. Implementors can override this if they
    /// need custom behavior.
    fn process_block(&mut self, ctx: &mut ProcessContext<T, BUF_SIZE>) -> ProcessResult<()>;
}

// ============================================================================
// Blanket Implementations for Trait Objects
// ============================================================================

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Source<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &mut ProcessContext<T, BUF_SIZE>) -> ProcessResult<()> {
        // Push clock tick telemetry — every Source runs on a fixed block
        // length whether driven by hardware or internal timing.
        if let Some(tx) = ctx.telemetry_tx {
            let beat = ctx.clock.beat_position().map(|b| b as f32).unwrap_or(0.0);
            let new_beat = if ctx.clock.is_new_beat() { 1.0 } else { 0.0 };
            let new_bar = if ctx.clock.is_new_bar() { 1.0 } else { 0.0 };
            tx.try_send(Telemetry::event(
                "source",
                CLOCK_TICK,
                vec![
                    ctx.clock.sample_pos as f32,
                    ctx.clock.sample_rate,
                    ctx.clock.tempo.unwrap_or(0.0),
                    beat,
                    new_beat,
                    new_bar,
                ],
            ));
        }
        self.as_mut()
            .generate(ctx.clock, ctx.control_inputs, ctx.clock_inputs)
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Processor<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &mut ProcessContext<T, BUF_SIZE>) -> ProcessResult<()> {
        self.as_mut().process(
            ctx.clock,
            ctx.signal_inputs,
            ctx.control_inputs,
            ctx.clock_inputs,
            ctx.feedback_inputs,
        )
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Sink<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &mut ProcessContext<T, BUF_SIZE>) -> ProcessResult<()> {
        self.as_mut().consume(
            ctx.clock,
            ctx.signal_inputs,
            ctx.control_inputs,
            ctx.clock_inputs,
            ctx.feedback_inputs,
        )
    }
}

// ============================================================================
// NodeVariant Enum
// ============================================================================

/// Enum that holds any kind of audio node.
pub enum NodeVariant<T: Transcendental, const BUF_SIZE: usize> {
    Source(Box<dyn crate::traits::Source<T, BUF_SIZE>>),
    Processor(Box<dyn crate::traits::Processor<T, BUF_SIZE>>),
    Sink(Box<dyn crate::traits::Sink<T, BUF_SIZE>>),
}

impl<T: Transcendental, const BUF_SIZE: usize> Processable<T, BUF_SIZE> for NodeVariant<T, BUF_SIZE> {
    fn process_block(&mut self, ctx: &mut ProcessContext<T, BUF_SIZE>) -> ProcessResult<()> {
        match self {
            NodeVariant::Source(src) => src.process_block(ctx),
            NodeVariant::Processor(proc) => proc.process_block(ctx),
            NodeVariant::Sink(sink) => sink.process_block(ctx),
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> NodeVariant<T, BUF_SIZE> {
    /// Set a pointer to a shared tape loop.
    /// Delegates to the concrete node type (no-op on most nodes).
    #[allow(unsafe_code)]
    pub fn set_tape(&mut self, ptr: *const crate::buffer::TapeLoop<T>) {
        match self {
            NodeVariant::Source(src) => src.set_tape(ptr),
            NodeVariant::Processor(proc) => proc.set_tape(ptr),
            NodeVariant::Sink(sink) => sink.set_tape(ptr),
        }
    }

    /// Attach a telemetry sender to this node.
    pub fn set_telemetry_tx(&mut self, tx: TelemetryTx) {
        match self {
            NodeVariant::Source(src) => SignalNode::set_telemetry_tx(src.as_mut(), tx),
            NodeVariant::Processor(proc) => SignalNode::set_telemetry_tx(proc.as_mut(), tx),
            NodeVariant::Sink(sink) => SignalNode::set_telemetry_tx(sink.as_mut(), tx),
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> crate::traits::SignalNode<T, BUF_SIZE>
    for NodeVariant<T, BUF_SIZE>
{
    fn metadata(&self) -> crate::traits::NodeMetadata {
        match self {
            NodeVariant::Source(src) => src.metadata(),
            NodeVariant::Processor(proc) => proc.metadata(),
            NodeVariant::Sink(sink) => sink.metadata(),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        match self {
            NodeVariant::Source(src) => src.init(sample_rate),
            NodeVariant::Processor(proc) => proc.init(sample_rate),
            NodeVariant::Sink(sink) => sink.init(sample_rate),
        }
    }

    fn reset(&mut self) {
        match self {
            NodeVariant::Source(src) => src.reset(),
            NodeVariant::Processor(proc) => proc.reset(),
            NodeVariant::Sink(sink) => sink.reset(),
        }
    }

    fn get_parameter(&self, id: &crate::traits::ParameterId) -> Option<crate::traits::ParamValue> {
        match self {
            NodeVariant::Source(src) => src.get_parameter(id),
            NodeVariant::Processor(proc) => proc.get_parameter(id),
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
            NodeVariant::Sink(sink) => sink.set_parameter(id, value),
        }
    }

    fn id(&self) -> crate::traits::NodeId {
        match self {
            NodeVariant::Source(src) => src.id(),
            NodeVariant::Processor(proc) => proc.id(),
            NodeVariant::Sink(sink) => sink.id(),
        }
    }

    fn set_id(&mut self, id: crate::traits::NodeId) {
        match self {
            NodeVariant::Source(src) => src.set_id(id),
            NodeVariant::Processor(proc) => proc.set_id(id),
            NodeVariant::Sink(sink) => sink.set_id(id),
        }
    }

    fn num_signal_inputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**src;
                n.num_signal_inputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**proc;
                n.num_signal_inputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**sink;
                n.num_signal_inputs()
            }
        }
    }

    fn num_signal_outputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**src;
                n.num_signal_outputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**proc;
                n.num_signal_outputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**sink;
                n.num_signal_outputs()
            }
        }
    }

    fn num_control_inputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**src;
                n.num_control_inputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**proc;
                n.num_control_inputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**sink;
                n.num_control_inputs()
            }
        }
    }

    fn num_control_outputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**src;
                n.num_control_outputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**proc;
                n.num_control_outputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**sink;
                n.num_control_outputs()
            }
        }
    }

    fn num_clock_inputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**src;
                n.num_clock_inputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**proc;
                n.num_clock_inputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**sink;
                n.num_clock_inputs()
            }
        }
    }

    fn num_clock_outputs(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**src;
                n.num_clock_outputs()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**proc;
                n.num_clock_outputs()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**sink;
                n.num_clock_outputs()
            }
        }
    }

    fn num_feedback_ports(&self) -> usize {
        match self {
            NodeVariant::Source(src) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**src;
                n.num_feedback_ports()
            }
            NodeVariant::Processor(proc) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**proc;
                n.num_feedback_ports()
            }
            NodeVariant::Sink(sink) => {
                let n: &dyn crate::traits::SignalNode<T, BUF_SIZE> = &**sink;
                n.num_feedback_ports()
            }
        }
    }

    fn input_port(&self, index: usize) -> Option<&crate::traits::port::Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.input_port(index),
            NodeVariant::Processor(proc) => proc.input_port(index),
            NodeVariant::Sink(sink) => sink.input_port(index),
        }
    }

    fn input_port_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut crate::traits::port::Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.input_port_mut(index),
            NodeVariant::Processor(proc) => proc.input_port_mut(index),
            NodeVariant::Sink(sink) => sink.input_port_mut(index),
        }
    }

    fn output_port(&self, index: usize) -> Option<&crate::traits::port::Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.output_port(index),
            NodeVariant::Processor(proc) => proc.output_port(index),
            NodeVariant::Sink(sink) => sink.output_port(index),
        }
    }

    fn output_port_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut crate::traits::port::Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.output_port_mut(index),
            NodeVariant::Processor(proc) => proc.output_port_mut(index),
            NodeVariant::Sink(sink) => sink.output_port_mut(index),
        }
    }

    fn control_port(&self, index: usize) -> Option<&crate::traits::port::Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.control_port(index),
            NodeVariant::Processor(proc) => proc.control_port(index),
            NodeVariant::Sink(sink) => sink.control_port(index),
        }
    }

    fn control_port_mut(
        &mut self,
        index: usize,
    ) -> Option<&mut crate::traits::port::Port<T, BUF_SIZE>> {
        match self {
            NodeVariant::Source(src) => src.control_port_mut(index),
            NodeVariant::Processor(proc) => proc.control_port_mut(index),
            NodeVariant::Sink(sink) => sink.control_port_mut(index),
        }
    }

    fn state(&self) -> &crate::traits::NodeState<T, BUF_SIZE> {
        match self {
            NodeVariant::Source(src) => src.state(),
            NodeVariant::Processor(proc) => proc.state(),
            NodeVariant::Sink(sink) => sink.state(),
        }
    }

    fn state_mut(&mut self) -> &mut crate::traits::NodeState<T, BUF_SIZE> {
        match self {
            NodeVariant::Source(src) => src.state_mut(),
            NodeVariant::Processor(proc) => proc.state_mut(),
            NodeVariant::Sink(sink) => sink.state_mut(),
        }
    }
}
