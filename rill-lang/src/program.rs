//! `RillProgram<T>` — a compiled rill-lang program that implements
//! [`rill_core::Algorithm`]. Owns its IR and pre-allocated state; `process()`
//! performs no heap allocation.

use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ProcessResult};

use crate::ir::Ir;

/// A compiled program ready to run inside the rill graph.
pub struct RillProgram<T: Transcendental> {
    pub(crate) ir: Ir,
    /// Persistent feedback state (previous-sample values). Length = state_slots.
    pub(crate) state: Vec<f64>,
    /// Next-sample feedback writes, applied at sample end.
    pub(crate) state_next: Vec<f64>,
    /// Delay lines: ring buffers, one per `@` site.
    pub(crate) delays: Vec<DelayRing>,
    /// Scratch register file (reused each sample; sized once).
    pub(crate) regs: Vec<f64>,
    _marker: core::marker::PhantomData<T>,
}

/// A fixed-length ring buffer for one `@` delay site.
pub(crate) struct DelayRing {
    pub(crate) buf: Vec<f64>,
    pub(crate) head: usize,
}

impl DelayRing {
    pub(crate) fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            head: 0,
        }
    }
    /// The value `len` samples ago (the oldest in the ring).
    pub(crate) fn read(&self) -> f64 {
        self.buf[self.head]
    }
    /// Push a new sample, overwriting the oldest.
    pub(crate) fn write(&mut self, v: f64) {
        self.buf[self.head] = v;
        self.head = (self.head + 1) % self.buf.len();
    }
}

impl<T: Transcendental> RillProgram<T> {
    pub(crate) fn new(ir: Ir) -> Self {
        let state = vec![0.0; ir.state.state_slots];
        let state_next = state.clone();
        let delays = ir
            .state
            .delay_lens
            .iter()
            .map(|&l| DelayRing::new(l))
            .collect();
        let regs = vec![0.0; ir.num_regs];
        Self {
            ir,
            state,
            state_next,
            delays,
            regs,
            _marker: core::marker::PhantomData,
        }
    }
}

impl<T: Transcendental> Algorithm<T> for RillProgram<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        crate::backend::interp::run_block(self, input, output);
        Ok(())
    }

    fn reset(&mut self) {
        for s in &mut self.state {
            *s = 0.0;
        }
        for s in &mut self.state_next {
            *s = 0.0;
        }
        for d in &mut self.delays {
            for v in &mut d.buf {
                *v = 0.0;
            }
            d.head = 0;
        }
    }
}
