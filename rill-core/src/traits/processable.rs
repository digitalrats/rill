//! Processable trait for unified audio node processing.
//!
//! This module defines the `Processable` trait that unifies `generate`, `process`, and `consume`
//! into a single `process_block` method, making it easier to build generic audio graphs.

use crate::math::AudioNum;
use crate::time::ClockTick;
use crate::traits::ProcessResult;

// ============================================================================
// ProcessContext
// ============================================================================

/// Convenience structure that gathers all input/output buffers for a node.
pub struct ProcessContext<'a, T: AudioNum, const BUF_SIZE: usize> {
    /// Current clock tick
    pub clock: &'a ClockTick,
    /// Audio input buffers (slice of references to [T; BUF_SIZE])
    pub audio_inputs: &'a [&'a [T; BUF_SIZE]],
    /// Control input values (slice of T)
    pub control_inputs: &'a [T],
    /// Clock input ticks
    pub clock_inputs: &'a [ClockTick],
    /// Feedback input buffers (slice of references to [T; BUF_SIZE])
    pub feedback_inputs: &'a [&'a [T; BUF_SIZE]],
    /// Audio output buffers (slice of mutable references to [T; BUF_SIZE])
    pub audio_outputs: &'a mut [&'a mut [T; BUF_SIZE]],
    /// Control output values (slice of mutable T)
    pub control_outputs: &'a mut [T],
    /// Clock output ticks
    pub clock_outputs: &'a mut [ClockTick],
    /// Feedback output buffers (slice of mutable references to [T; BUF_SIZE])
    pub feedback_outputs: &'a mut [&'a mut [T; BUF_SIZE]],
}

// ============================================================================
// Processable Trait
// ============================================================================

/// Unified trait for processing audio nodes.
///
/// This trait is implemented for all `Source`, `Processor`, and `Sink` types,
/// providing a single method that dispatches to the appropriate underlying
/// method (`generate`, `process`, or `consume`).
pub trait Processable<T: AudioNum, const BUF_SIZE: usize> {
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

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE> for Box<dyn crate::traits::Source<T, BUF_SIZE>>
where
    T: AudioNum,
{
    fn process_block(&mut self, ctx: &mut ProcessContext<T, BUF_SIZE>) -> ProcessResult<()> {
        self.as_mut().generate(
            ctx.clock,
            ctx.control_inputs,
            ctx.clock_inputs,
            ctx.audio_outputs,
        )
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE> for Box<dyn crate::traits::Processor<T, BUF_SIZE>>
where
    T: AudioNum,
{
    fn process_block(&mut self, ctx: &mut ProcessContext<T, BUF_SIZE>) -> ProcessResult<()> {
        self.as_mut().process(
            ctx.clock,
            ctx.audio_inputs,
            ctx.control_inputs,
            ctx.clock_inputs,
            ctx.feedback_inputs,
            ctx.audio_outputs,
            ctx.control_outputs,
            ctx.clock_outputs,
            ctx.feedback_outputs,
        )
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE> for Box<dyn crate::traits::Sink<T, BUF_SIZE>>
where
    T: AudioNum,
{
    fn process_block(&mut self, ctx: &mut ProcessContext<T, BUF_SIZE>) -> ProcessResult<()> {
        self.as_mut().consume(
            ctx.clock,
            ctx.audio_inputs,
            ctx.control_inputs,
            ctx.clock_inputs,
            ctx.feedback_inputs,
            ctx.control_outputs,
            ctx.clock_outputs,
        )
    }
}

// ============================================================================
// NodeVariant Enum
// ============================================================================

/// Enum that holds any kind of audio node.
pub enum NodeVariant<T: AudioNum, const BUF_SIZE: usize> {
    Source(Box<dyn crate::traits::Source<T, BUF_SIZE>>),
    Processor(Box<dyn crate::traits::Processor<T, BUF_SIZE>>),
    Sink(Box<dyn crate::traits::Sink<T, BUF_SIZE>>),
}

impl<T: AudioNum, const BUF_SIZE: usize> Processable<T, BUF_SIZE> for NodeVariant<T, BUF_SIZE> {
    fn process_block(&mut self, ctx: &mut ProcessContext<T, BUF_SIZE>) -> ProcessResult<()> {
        match self {
            NodeVariant::Source(src) => src.process_block(ctx),
            NodeVariant::Processor(proc) => proc.process_block(ctx),
            NodeVariant::Sink(sink) => sink.process_block(ctx),
        }
    }
}