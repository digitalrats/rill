//! `RillProgram<T>` — a compiled rill-lang program that implements
//! [`rill_core::Algorithm`]. Owns its IR, schedule, and pre-allocated state;
//! `process()` performs no heap allocation after warm-up.

use rill_core::math::Transcendental;
use rill_core::traits::{Algorithm, ProcessResult};

use crate::builtin::SampleBuiltin;
use crate::error::CompileError;
use crate::ir::Ir;
use crate::schedule::{build_schedule, Schedule};

/// A runtime built-in instance, indexed directly by IR `instance` fields.
pub(crate) enum BuiltinInst<T: Transcendental> {
    /// A per-sample stateful built-in.
    Sample(Box<dyn SampleBuiltin<T>>),
    /// An opaque whole-buffer built-in.
    Block(Box<dyn Algorithm<T>>),
}

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
    /// Runtime built-in instances (indexed by `ir.builtins` indices).
    pub(crate) builtins: Vec<BuiltinInst<T>>,
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
            builtins: Vec::new(),
        }
    }

    pub(crate) fn new_with(
        ir: Ir,
        registry: &crate::builtin::Registry<T>,
        sample_rate: f32,
    ) -> Result<Self, CompileError> {
        let mut builtins = Vec::with_capacity(ir.builtins.len());
        for bi in &ir.builtins {
            let entry = registry.get(&bi.name).ok_or_else(|| {
                CompileError::Unsupported(format!("unknown built-in '{}'", bi.name))
            })?;
            match bi.kind {
                crate::builtin::BuiltinKind::Sample => {
                    let mut b = entry
                        .build_sample(&bi.params, sample_rate)
                        .expect("registry build_sample failed for sample builtin");
                    b.init(sample_rate);
                    builtins.push(BuiltinInst::Sample(b));
                }
                crate::builtin::BuiltinKind::Block => {
                    let mut b = entry
                        .build_block(&bi.params, sample_rate)
                        .expect("registry build_block failed for block builtin");
                    Algorithm::init(b.as_mut(), sample_rate);
                    builtins.push(BuiltinInst::Block(b));
                }
            }
        }
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
        Ok(Self {
            ir,
            schedule,
            state,
            state_next,
            delays,
            block_regs,
            regs_scalar,
            builtins,
        })
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

    /// Forward initialisation to all built-in instances.
    pub fn init(&mut self, sample_rate: f32) {
        for b in &mut self.builtins {
            match b {
                BuiltinInst::Sample(inst) => inst.init(sample_rate),
                BuiltinInst::Block(inst) => Algorithm::init(inst.as_mut(), sample_rate),
            }
        }
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
        for b in &mut self.builtins {
            match b {
                BuiltinInst::Sample(inst) => inst.reset(),
                BuiltinInst::Block(inst) => Algorithm::reset(inst.as_mut()),
            }
        }
    }
}
