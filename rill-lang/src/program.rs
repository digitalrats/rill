//! `RillProgram<T>` — a compiled rill-lang program that implements
//! [`rill_core::Algorithm`]. Owns its IR, schedule, and pre-allocated state;
//! `process()` performs no heap allocation after warm-up.

use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ProcessResult};

use crate::ir::Ir;
use crate::schedule::{build_schedule, Schedule};

/// A compiled program ready to run inside the rill graph.
pub struct RillProgram<T: Transcendental> {
    pub(crate) ir: Ir,
    pub(crate) schedule: Schedule,
    /// Persistent feedback state (previous-sample values). Length = state_slots.
    pub(crate) state: Vec<f64>,
    /// Next-sample feedback writes, applied at sample end.
    pub(crate) state_next: Vec<f64>,
    /// Delay lines: ring buffers, one per `@` site.
    pub(crate) delays: Vec<DelayRing>,
    /// Whole-buffer register store for the hybrid path (grown to block length).
    pub(crate) block_regs: Vec<Vec<T>>,
    /// Scalar register file for the reference (per-sample) path.
    pub(crate) regs_scalar: Vec<f64>,
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
    pub(crate) fn read(&self) -> f64 {
        self.buf[self.head]
    }
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
        let block_regs = vec![Vec::new(); ir.num_regs];
        let regs_scalar = vec![0.0; ir.num_regs];
        let schedule = build_schedule(&ir);
        Self {
            ir,
            schedule,
            state,
            state_next,
            delays,
            block_regs,
            regs_scalar,
        }
    }

    /// Ensure every block register can hold `n` samples (grows + reuses).
    pub(crate) fn ensure_block_len(&mut self, n: usize) {
        for r in &mut self.block_regs {
            if r.len() < n {
                r.resize(n, T::ZERO);
            }
        }
    }

    /// Reference implementation: the MVP per-sample interpreter. Used by tests
    /// as a numerical oracle; not the production path.
    pub fn process_reference(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
    ) -> ProcessResult<()> {
        crate::backend::interp::run_block_reference(self, input, output);
        Ok(())
    }
}

impl<T: Transcendental> Algorithm<T> for RillProgram<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        crate::backend::interp::run_block_hybrid(self, input, output);
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
