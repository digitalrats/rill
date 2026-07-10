//! Processable trait for block-level signal processing.

use crate::math::Transcendental;
use crate::time::{ClockTick, RenderContext};
use crate::traits::ProcessResult;

// ============================================================================
// Processable Trait
// ============================================================================

/// A node or component that can process a block of signal data.
///
/// This is the main execution trait for the signal graph. Each node type
/// (Source, Processor, Router, Sink) implements this via blanket impls
/// that delegate to their respective `generate`/`process`/`route`/`consume`.
pub trait Processable<T: Transcendental, const BUF_SIZE: usize> {
    /// Process one block of signal samples.
    ///
    /// # Arguments
    ///
    /// * `ctx` — [`RenderContext`] with sample clock, transport state, and
    ///   hardware clock correction.
    /// * `tick` — [`ClockTick`] with timing metadata (sample position,
    ///   rate, speed_ratio). I/O access is through `IoCapture` / `IoPlayback`
    ///   traits held by Source / Sink nodes. Processor and Router ignore the tick.
    ///
    /// # Errors
    /// Returns a [`ProcessError`](crate::traits::ProcessError) if processing fails.
    fn process_block(&mut self, ctx: &RenderContext, tick: &ClockTick) -> ProcessResult<()>;
}

// ============================================================================
// Blanket Implementations
// ============================================================================

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Source<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &RenderContext, tick: &ClockTick) -> ProcessResult<()> {
        const {
            assert!(
                BUF_SIZE.is_multiple_of(4),
                "BUF_SIZE must be a multiple of 4 for SIMD"
            )
        }
        self.as_mut().generate(ctx, &[], &[], tick)
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Processor<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &RenderContext, _tick: &ClockTick) -> ProcessResult<()> {
        self.as_mut().process(ctx, &[], &[], &[], &[])
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Sink<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &RenderContext, tick: &ClockTick) -> ProcessResult<()> {
        self.as_mut().consume(ctx, &[], &[], &[], &[], tick)
    }
}

impl<T, const BUF_SIZE: usize> Processable<T, BUF_SIZE>
    for Box<dyn crate::traits::Router<T, BUF_SIZE>>
where
    T: Transcendental,
{
    fn process_block(&mut self, ctx: &RenderContext, _tick: &ClockTick) -> ProcessResult<()> {
        (**self).route(ctx, &[])
    }
}
