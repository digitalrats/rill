//! `RillProgram<T>` — a compiled rill-lang program that implements
//! [`rill_core::Algorithm`]. Owns its IR, schedule, and pre-allocated state;
//! `process()` performs no heap allocation after warm-up.

use rill_core::math::Transcendental;
#[cfg(feature = "router")]
use rill_core::traits::MultichannelAlgorithm;
use rill_core::traits::{Algorithm, ParamValue, ProcessResult};

use crate::builtin::{BlockBuiltin, SampleBuiltin};
use crate::error::CompileError;
use crate::ir::{Ir, ParamDef};
use crate::schedule::{build_schedule, Schedule};

/// A runtime built-in instance, indexed directly by IR `instance` fields.
pub(crate) enum BuiltinInst<T: Transcendental> {
    /// A per-sample stateful built-in.
    Sample(Box<dyn SampleBuiltin<T>>),
    /// An opaque whole-buffer built-in.
    Block(Box<dyn BlockBuiltin<T>>),
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
    /// Current parameter values, indexed by [`Ir::params`].
    pub(crate) params: Vec<ParamValue>,
    /// Dirty flags: true when a param was changed since last push.
    pub(crate) params_dirty: Vec<bool>,
    /// Parameter metadata (name, default, range).
    pub(crate) params_meta: Vec<ParamDef>,
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
    pub fn new(ir: Ir) -> Self {
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
            state,
            state_next,
            delays,
            block_regs,
            regs_scalar,
            builtins: Vec::new(),
            params,
            params_dirty,
            params_meta,
        }
    }

    /// Create a program from a compiled [`Ir`], instantiating all built-ins
    /// via the provided [`Registry`]. Also sets the initial `sample_rate`.
    ///
    /// Parses `builtins` from the IR, allocates registers, state, and delays,
    /// and builds the execution schedule. The resulting program implements
    /// [`Algorithm<T>`](rill_core::traits::Algorithm).
    pub fn new_with(
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
            state,
            state_next,
            delays,
            block_regs,
            regs_scalar,
            builtins,
            params,
            params_dirty,
            params_meta,
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

#[cfg(feature = "router")]
impl<T: Transcendental> MultichannelAlgorithm<T> for RillProgram<T> {
    fn num_inputs(&self) -> usize {
        self.ir.num_inputs
    }

    fn num_outputs(&self) -> usize {
        self.ir.num_outputs
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        let n_in = inputs.len();
        let n_out = outputs.len();
        let buf_size = if n_out > 0 { outputs[0].len() } else { 0 };

        if n_in <= 1 && n_out == 1 {
            let input = if n_in == 0 { None } else { Some(inputs[0]) };
            return Algorithm::process(self, input, outputs[0]);
        }

        crate::backend::interp::push_builtin_params(self);
        for sample_idx in 0..buf_size {
            let in_sample = if n_in > 0 {
                inputs[0][sample_idx].to_f64()
            } else {
                0.0
            };
            let y = crate::backend::interp::eval_sample_scalar(self, in_sample);
            if n_out > 0 {
                outputs[0][sample_idx] = T::from_f64(y);
            }
        }
        Ok(())
    }

    fn reset(&mut self) {
        Algorithm::reset(self);
    }
}
