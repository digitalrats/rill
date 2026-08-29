//! `RillProgram<T>` — a compiled rill-lang program that implements
//! [`rill_core::Algorithm`]. Owns its IR, schedule, and pre-allocated state;
//! `process()` performs no heap allocation after warm-up.

use rill_core::builtin::MultichannelBlockBuiltin;
use rill_core::math::Transcendental;
use rill_core::traits::MultichannelAlgorithm;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};

use crate::builtin::BlockBuiltin;
use crate::error::CompileError;
use crate::ir::{Ir, ParamDef};
use crate::schedule::{build_schedule, Schedule};

/// A runtime built-in instance, indexed directly by IR `instance` fields.
pub(crate) enum BuiltinInst<T: Transcendental> {
    /// An opaque whole-buffer built-in.
    Block(Box<dyn BlockBuiltin<T>>),
    /// A whole-buffer multi-channel built-in.
    MultichannelBlock(Box<dyn MultichannelBlockBuiltin<T>>),
}

/// A compiled program ready to run inside the rill graph.
pub struct RillProgram<T: Transcendental> {
    pub(crate) ir: Ir,
    pub(crate) schedule: Schedule,
    /// Block-level feedback state (previous tick's whole block per slot).
    pub(crate) block_state: Vec<Vec<T>>,
    /// Current-tick feedback writes, swapped with `block_state` at tick end.
    pub(crate) block_state_next: Vec<Vec<T>>,
    /// Delay lines: block-level ring buffers, one per `@` site.
    pub(crate) delays: Vec<DelayRing<T>>,
    /// Whole-buffer register store (grown to block length).
    pub(crate) block_regs: Vec<Vec<T>>,
    /// Runtime built-in instances (indexed by `ir.builtins` indices).
    pub(crate) builtins: Vec<BuiltinInst<T>>,
    /// Current parameter values, indexed by [`Ir::params`].
    pub(crate) params: Vec<ParamValue>,
    /// Dirty flags: true when a param was changed since last push.
    pub(crate) params_dirty: Vec<bool>,
    /// Parameter metadata (name, default, range).
    pub(crate) params_meta: Vec<ParamDef>,
}

/// A fixed-length ring buffer for one `@` delay site, processed whole-block.
pub(crate) struct DelayRing<T> {
    buf: Vec<T>,
    head: usize,
    len: usize,
}

impl<T: Transcendental> DelayRing<T> {
    pub(crate) fn new(len: usize) -> Self {
        let len = len.max(1);
        Self {
            buf: vec![T::ZERO; len],
            head: 0,
            len,
        }
    }

    /// Read a whole block of delayed samples into `out`.
    pub(crate) fn read_block(&self, out: &mut [T]) {
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.buf[(self.head + i) % self.len];
        }
    }

    /// Write a whole block into the ring buffer and advance the head.
    pub(crate) fn write_block(&mut self, input: &[T]) {
        for (i, &v) in input.iter().enumerate() {
            self.buf[(self.head + i) % self.len] = v;
        }
        self.head = (self.head + input.len()) % self.len;
    }
}

impl<T: Transcendental> RillProgram<T> {
    /// Create a program from a compiled IR. Allocates state, delays, registers,
    /// and builds the execution schedule. Built-ins are NOT instantiated — use
    /// [`new_with`](Self::new_with) if the IR references built-in functions.
    pub fn new(ir: Ir) -> Self {
        let block_state = vec![Vec::new(); ir.state.block_state_slots];
        let block_state_next = vec![Vec::new(); ir.state.block_state_slots];
        let delays = ir
            .state
            .delay_lens
            .iter()
            .map(|&l| DelayRing::new(l))
            .collect();
        let block_regs = vec![Vec::new(); ir.num_regs];
        let schedule = build_schedule(&ir);
        let params_meta = ir.params.clone();
        let params: Vec<ParamValue> = ir
            .params
            .iter()
            .map(|p| ParamValue::Float(p.default as f32))
            .collect();
        let params_dirty = vec![false; params.len()];
        Self {
            ir,
            schedule,
            block_state,
            block_state_next,
            delays,
            block_regs,
            builtins: Vec::new(),
            params,
            params_dirty,
            params_meta,
        }
    }

    /// Create a program from a compiled [`Ir`], instantiating all built-ins
    /// via the provided `Registry`. Also sets the initial `sample_rate`.
    ///
    /// Parses `builtins` from the IR, allocates registers, state, and delays,
    /// and builds the execution schedule. The resulting program implements
    /// [`Algorithm<T>`](rill_core::traits::Algorithm).
    pub fn new_with(
        ir: Ir,
        registry: &crate::builtin::Registry<T>,
        sample_rate: f32,
    ) -> Result<Self, CompileError> {
        Self::build(ir, registry, sample_rate)
    }

    fn build(
        ir: Ir,
        registry: &crate::builtin::Registry<T>,
        sample_rate: f32,
    ) -> Result<Self, CompileError> {
        let mut builtins = Vec::with_capacity(ir.builtins.len());
        for bi in &ir.builtins {
            let entry = registry.get(&bi.name).ok_or_else(|| {
                CompileError::Unsupported(format!("unknown built-in '{}'", bi.name))
            })?;
            let is_multi = bi.signal_ins > 1 || bi.signal_outs > 1;
            if is_multi {
                let mut b: Box<dyn MultichannelBlockBuiltin<T>> = entry
                    .build_multichannel_block(bi.signal_ins, &bi.params, sample_rate)
                    .expect("registry build_multichannel_block failed");
                MultichannelAlgorithm::reset(b.as_mut());
                builtins.push(BuiltinInst::MultichannelBlock(b));
            } else {
                let mut b: Box<dyn BlockBuiltin<T>> = entry
                    .build_block(&bi.params, sample_rate)
                    .expect("registry build_block failed for block builtin");
                Algorithm::init(b.as_mut(), sample_rate);
                builtins.push(BuiltinInst::Block(b));
            }
        }

        let block_state = vec![Vec::new(); ir.state.block_state_slots];
        let block_state_next = vec![Vec::new(); ir.state.block_state_slots];
        let delays = ir
            .state
            .delay_lens
            .iter()
            .map(|&l| DelayRing::new(l))
            .collect();
        let block_regs = vec![Vec::new(); ir.num_regs];
        let schedule = build_schedule(&ir);
        let params_meta = ir.params.clone();
        let params: Vec<ParamValue> = ir
            .params
            .iter()
            .map(|p| ParamValue::Float(p.default as f32))
            .collect();
        let params_dirty = vec![false; params.len()];
        Ok(Self {
            ir,
            schedule,
            block_state,
            block_state_next,
            delays,
            block_regs,
            builtins,
            params,
            params_dirty,
            params_meta,
        })
    }

    /// Ensure every block register and block-state buffer can hold `n` samples.
    pub(crate) fn ensure_block_len(&mut self, n: usize) {
        for r in &mut self.block_regs {
            if r.len() < n {
                r.resize(n, T::ZERO);
            }
        }
        for b in &mut self.block_state {
            if b.len() < n {
                b.resize(n, T::ZERO);
            }
        }
        for b in &mut self.block_state_next {
            if b.len() < n {
                b.resize(n, T::ZERO);
            }
        }
    }

    /// Swap the double-buffered block feedback state at the end of a tick.
    pub(crate) fn swap_block_state(&mut self) {
        std::mem::swap(&mut self.block_state, &mut self.block_state_next);
        for b in &mut self.block_state_next {
            b.fill(T::ZERO);
        }
    }

    /// Index of a named parameter, if present.
    pub fn param_index(&self, name: &str) -> Option<usize> {
        self.params_meta.iter().position(|p| p.name == name)
    }

    /// Set a parameter by index. RT-safe (plain store).
    pub fn set_param(&mut self, idx: usize, value: ParamValue) {
        if let Some(def) = self.params_meta.get(idx) {
            let clamped = match &value {
                ParamValue::Float(v) => {
                    ParamValue::Float((*v as f64).clamp(def.min, def.max) as f32)
                }
                ParamValue::Int(v) if *v as f64 >= def.min && (*v as f64) <= def.max => value,
                _ => value,
            };
            self.params[idx] = clamped;
            if let Some(d) = self.params_dirty.get_mut(idx) {
                *d = true;
            }
        }
    }

    /// Current value of a parameter by index.
    pub fn param(&self, idx: usize) -> ParamValue {
        self.params
            .get(idx)
            .cloned()
            .unwrap_or(ParamValue::Float(0.0))
    }

    /// Metadata for all parameters (name, default, range).
    pub fn params_meta(&self) -> &[ParamDef] {
        &self.params_meta
    }

    /// Forward initialisation to all built-in instances.
    pub fn init(&mut self, sample_rate: f32) {
        for b in &mut self.builtins {
            match b {
                BuiltinInst::Block(inst) => Algorithm::init(inst.as_mut(), sample_rate),
                BuiltinInst::MultichannelBlock(_) => {}
            }
        }
    }
}

impl<T: Transcendental> Algorithm<T> for RillProgram<T> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let inputs: &[&[T]] = if let Some(inp) = input { &[inp] } else { &[] };
        let mut outs: [&mut [T]; 1] = [output];
        crate::backend::interp::run_block_mimo(self, inputs, &mut outs);
        Ok(())
    }

    fn reset(&mut self) {
        for b in &mut self.block_state {
            b.fill(T::ZERO);
        }
        for b in &mut self.block_state_next {
            b.fill(T::ZERO);
        }
        for d in &mut self.delays {
            d.buf.fill(T::ZERO);
            d.head = 0;
        }
        for b in &mut self.builtins {
            match b {
                BuiltinInst::Block(inst) => Algorithm::reset(inst.as_mut()),
                BuiltinInst::MultichannelBlock(_) => {}
            }
        }
    }
}

impl<T: Transcendental> MultichannelAlgorithm<T> for RillProgram<T> {
    fn num_inputs(&self) -> usize {
        self.ir.num_inputs
    }

    fn num_outputs(&self) -> usize {
        self.ir.num_outputs
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        crate::backend::interp::run_block_mimo(self, inputs, outputs);
        Ok(())
    }

    fn reset(&mut self) {
        Algorithm::reset(self);
    }
}

impl<T: Transcendental> BlockBuiltin<T> for RillProgram<T> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        self.set_param(index, value.clone());
    }
}
