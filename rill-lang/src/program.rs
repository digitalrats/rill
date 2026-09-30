//! `RillProgram<T, BUF>` — a compiled rill-lang program that implements
//! [`rill_core::Algorithm`]. Owns its IR, schedule, and pre-allocated state;
//! `process()` performs no heap allocation after warm-up.

use rill_core::buffer::FixedBuffer;
use rill_core::builtin::MultichannelBlockBuiltin;
use rill_core::math::Transcendental;
use rill_core::traits::MultichannelAlgorithm;
use rill_core::traits::{Algorithm, ParamValue, ProcessError, ProcessResult};

use crate::arena::{Arena, BufferPool};
use crate::builtin::BlockBuiltin;
use crate::error::CompileError;
use crate::ir::{Ir, ParamDef, ValueBlock};
use crate::schedule::{build_schedule, Schedule};

/// Upper bound on a single `@ n` delay line, in samples.
///
/// Delay lengths are compile-time constants per site; every ring is a fixed
/// [`FixedBuffer`] of this size (largest delay allowed by the `@` operator).
pub(crate) const MAX_DELAY_LEN: usize = 65536;

/// Upper bound on built-in signal channels routed through one call site.
///
/// Used for stack scratch storage in the interpreter's foreign-block path.
pub(crate) const MAX_BUILTIN_CHANNELS: usize = 8;

/// A runtime built-in instance, indexed directly by IR `instance` fields.
pub(crate) enum BuiltinInst<T: Transcendental> {
    /// An opaque whole-buffer built-in.
    Block(Box<dyn BlockBuiltin<T>>),
    /// A whole-buffer multi-channel built-in.
    MultichannelBlock(Box<dyn MultichannelBlockBuiltin<T>>),
}

/// A compiled program ready to run inside the rill graph.
pub struct RillProgram<T: Transcendental, const BUF: usize> {
    pub(crate) ir: Ir,
    pub(crate) schedule: Schedule,
    /// Block-level feedback state (previous tick's whole block per slot).
    pub(crate) block_state: Vec<FixedBuffer<T, BUF>>,
    /// Current-tick feedback writes, swapped with `block_state` at tick end.
    pub(crate) block_state_next: Vec<FixedBuffer<T, BUF>>,
    /// Delay lines: block-level ring buffers, one per `@` site.
    pub(crate) delays: Vec<DelayRing<T, MAX_DELAY_LEN>>,
    /// Whole-buffer register store (fixed length `BUF` per register).
    pub(crate) block_regs: Vec<FixedBuffer<T, BUF>>,
    /// Runtime built-in instances (indexed by `ir.builtins` indices).
    pub(crate) builtins: Vec<BuiltinInst<T>>,
    /// Current parameter values, indexed by [`Ir::params`].
    pub(crate) params: Vec<ParamValue>,
    /// Dirty flags: true when a param was changed since last push.
    pub(crate) params_dirty: Vec<bool>,
    /// Parameter metadata (name, default, range).
    pub(crate) params_meta: Vec<ParamDef>,
    /// Value arena (fixed capacity from IR).
    pub(crate) arena: Arena,
    /// Persistent main λ-parameter cells, one per [`Ir::num_main_cells`]. Each
    /// cell is a `Void` arena slot allocated ONCE at construction — the value
    /// track never rebinds them, so a `SetParameter` write survives across
    /// ticks. The block track (`Instr::ReadMainCell`) and value track
    /// (`ValueReadMainCell`) read these directly.
    pub(crate) main_cells: Vec<Option<crate::arena::ArenaRef>>,
    /// Per-tick value registers. The store is pre-sized to
    /// `ir.num_value_regs + ir.max_call_regs`: the leading `num_value_regs`
    /// slots are the program's own registers, the tail is the pre-allocated
    /// function-call scratch that [`run_fragment`](crate::backend::interp::run_fragment)
    /// borrows per dispatch (see [`Self::value_regs_top`]). No reallocation on
    /// the RT path.
    pub(crate) value_regs: Vec<Option<crate::arena::ArenaRef>>,
    /// Watermark for the pre-allocated call scratch: the index one past the
    /// last occupied value register. Equals `ir.num_value_regs` outside a
    /// fragment dispatch and grows into the pre-allocated tail for the
    /// duration of each (possibly nested) call.
    pub(crate) value_regs_top: usize,
    /// Per-tick value-state slots (feedback/delay of values).
    pub(crate) value_state: Vec<Option<crate::arena::ArenaRef>>,
    /// Current runtime cell-stack frames (bindings).
    pub(crate) cell_stack: Vec<Vec<(u32, crate::arena::ArenaRef)>>,
    /// Shared capture-cell store for function fragments (see
    /// [`run_fragment`](crate::backend::interp::run_fragment)). Each dispatch
    /// appends its env-field capture cells at the current top; the fragment's
    /// `ValueReadCell { cell: i }` capture reads resolve to
    /// `frag_cells[frag_cells_base + i]` while a frame is active. Pre-sized at
    /// construction to the build-time cell bound so the RT path never
    /// reallocates.
    pub(crate) frag_cells: Vec<crate::arena::ArenaRef>,
    /// Base index of the currently-executing fragment's capture cells in
    /// [`Self::frag_cells`]; equals `frag_cells.len()` outside a dispatch.
    pub(crate) frag_cells_base: usize,
    /// Shared deferred-drop scratch: arena refs queued by value instructions
    /// and released at a well-defined point (end of the value track, end of a
    /// fragment dispatch). Pre-sized at construction; used as a stack (nested
    /// dispatches append above the caller's mark and drain on return).
    pub(crate) drops_scratch: Vec<crate::arena::ArenaRef>,
    /// Value outputs of the last processed tick: one counted arena ref per
    /// value output channel, held across ticks (see [`value_outputs`](Self::value_outputs)).
    pub(crate) value_outputs: Vec<Option<crate::arena::ArenaRef>>,
    /// Runtime value-track error (capacity overflow), set by a collection op
    /// and consumed at the end of the value phase. Cleared each tick.
    pub(crate) value_error: Option<ProcessError>,
}

/// A fixed-length ring buffer for one `@` delay site, processed whole-block.
///
/// Backed by a [`FixedBuffer`] (no heap in the RT path); the ring's usable
/// length is the site's compile-time delay length, clamped to `MAX_DELAY`.
pub(crate) struct DelayRing<T: Transcendental, const MAX_DELAY: usize> {
    buf: FixedBuffer<T, MAX_DELAY>,
    head: usize,
    len: usize,
}

impl<T: Transcendental, const MAX_DELAY: usize> DelayRing<T, MAX_DELAY> {
    pub(crate) fn new(len: usize) -> Self {
        assert!(
            (1..=MAX_DELAY).contains(&len),
            "delay length {len} exceeds MAX_DELAY_LEN ({MAX_DELAY})"
        );
        Self {
            buf: FixedBuffer::new(),
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

    /// Zero the ring and reset the head.
    pub(crate) fn clear(&mut self) {
        self.buf.fill(T::ZERO);
        self.head = 0;
    }
}

impl<T: Transcendental, const BUF: usize> RillProgram<T, BUF> {
    /// Create a program from a compiled IR. Allocates state, delays, registers,
    /// and builds the execution schedule. Built-ins are NOT instantiated — use
    /// [`new_with`](Self::new_with) if the IR references built-in functions.
    pub fn new(ir: Ir) -> Self {
        let block_state = vec![FixedBuffer::new(); ir.state.block_state_slots];
        let block_state_next = vec![FixedBuffer::new(); ir.state.block_state_slots];
        let delays = ir
            .state
            .delay_lens
            .iter()
            .map(|&l| DelayRing::new(l))
            .collect();
        let block_regs = vec![FixedBuffer::new(); ir.num_regs];
        let schedule = build_schedule(&ir);
        let params_meta = ir.params.clone();
        let params: Vec<ParamValue> = ir
            .params
            .iter()
            .map(|p| ParamValue::Float(p.default as f32))
            .collect();
        let params_dirty = vec![false; params.len()];
        let mut arena = Arena::with_capacity(ir.value_state.capacity);
        arena.pool = BufferPool::new(
            ir.value_state.buffer_budget,
            cfg!(feature = "growable-arena"),
        );
        let main_cells = Self::alloc_main_cells(&mut arena, ir.num_main_cells);
        // Pre-allocate the value-register store: the program's own registers
        // plus the function-call scratch (see `Ir::max_call_regs`).
        let value_regs = vec![None; ir.num_value_regs + ir.max_call_regs];
        let value_regs_top = ir.num_value_regs;
        let value_state = vec![None; ir.value_state.value_state_slots];
        let cell_stack = Vec::new();
        let frag_cells = Vec::with_capacity(Self::max_frag_cells(&ir));
        let drops_scratch = Vec::with_capacity(Self::max_drops(&ir));
        let value_outputs = vec![None; ir.value_output_regs.len()];
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
            arena,
            main_cells,
            value_regs,
            value_regs_top,
            value_state,
            cell_stack,
            frag_cells,
            frag_cells_base: 0,
            drops_scratch,
            value_outputs,
            value_error: None,
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
        Self::build(ir, registry, sample_rate, None)
    }

    /// Create a program with a resource registry, resolving resource-backed
    /// built-ins (e.g. tape heads) from the named resources.
    pub fn new_with_resources(
        ir: Ir,
        registry: &crate::builtin::Registry<T>,
        sample_rate: f32,
        resources: &mut rill_core::buffer::ResourceRegistry<T>,
    ) -> Result<Self, CompileError> {
        Self::build(ir, registry, sample_rate, Some(resources))
    }

    fn build(
        ir: Ir,
        registry: &crate::builtin::Registry<T>,
        sample_rate: f32,
        mut resources: Option<&mut rill_core::buffer::ResourceRegistry<T>>,
    ) -> Result<Self, CompileError> {
        let mut builtins = Vec::with_capacity(ir.builtins.len());
        for bi in &ir.builtins {
            let entry = registry.get(&bi.name).ok_or_else(|| {
                CompileError::Unsupported(format!("unknown built-in '{}'", bi.name))
            })?;
            let is_multi = bi.signal_ins > 1 || bi.signal_outs > 1;
            if is_multi {
                let mut b: Box<dyn MultichannelBlockBuiltin<T>> = if let Some(res) = &bi.resource {
                    let reg = resources.as_deref_mut().ok_or_else(|| {
                        CompileError::Unsupported(format!(
                            "built-in '{}' requires a resource registry",
                            bi.name
                        ))
                    })?;
                    entry
                        .build_resource_multichannel_block(
                            bi.signal_ins,
                            &bi.params,
                            sample_rate,
                            reg,
                            res,
                        )
                        .ok_or_else(|| {
                            CompileError::Unsupported(format!(
                                "resource built-in '{}' is not registered as resource-backed",
                                bi.name
                            ))
                        })?
                } else {
                    entry
                        .build_multichannel_block(bi.signal_ins, &bi.params, sample_rate)
                        .expect("registry build_multichannel_block failed")
                };
                MultichannelAlgorithm::reset(b.as_mut());
                builtins.push(BuiltinInst::MultichannelBlock(b));
            } else {
                let mut b: Box<dyn BlockBuiltin<T>> = if let Some(res) = &bi.resource {
                    let reg = resources.as_deref_mut().ok_or_else(|| {
                        CompileError::Unsupported(format!(
                            "built-in '{}' requires a resource registry",
                            bi.name
                        ))
                    })?;
                    entry
                        .build_resource_block(&bi.params, sample_rate, reg, res)
                        .ok_or_else(|| {
                            CompileError::Unsupported(format!(
                                "resource built-in '{}' is not registered as resource-backed",
                                bi.name
                            ))
                        })?
                } else {
                    entry
                        .build_block(&bi.params, sample_rate)
                        .expect("registry build_block failed for block builtin")
                };
                Algorithm::init(b.as_mut(), sample_rate);
                builtins.push(BuiltinInst::Block(b));
            }
        }

        let block_state = vec![FixedBuffer::new(); ir.state.block_state_slots];
        let block_state_next = vec![FixedBuffer::new(); ir.state.block_state_slots];
        let delays = ir
            .state
            .delay_lens
            .iter()
            .map(|&l| DelayRing::new(l))
            .collect();
        let block_regs = vec![FixedBuffer::new(); ir.num_regs];
        let schedule = build_schedule(&ir);
        let params_meta = ir.params.clone();
        let params: Vec<ParamValue> = ir
            .params
            .iter()
            .map(|p| ParamValue::Float(p.default as f32))
            .collect();
        let params_dirty = vec![false; params.len()];
        let mut arena = Arena::with_capacity(ir.value_state.capacity);
        arena.pool = BufferPool::new(
            ir.value_state.buffer_budget,
            cfg!(feature = "growable-arena"),
        );
        let main_cells = Self::alloc_main_cells(&mut arena, ir.num_main_cells);
        // Pre-allocate the value-register store: the program's own registers
        // plus the function-call scratch (see `Ir::max_call_regs`).
        let value_regs = vec![None; ir.num_value_regs + ir.max_call_regs];
        let value_regs_top = ir.num_value_regs;
        let value_state = vec![None; ir.value_state.value_state_slots];
        let cell_stack = Vec::new();
        let frag_cells = Vec::with_capacity(Self::max_frag_cells(&ir));
        let drops_scratch = Vec::with_capacity(Self::max_drops(&ir));
        let value_outputs = vec![None; ir.value_output_regs.len()];
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
            arena,
            main_cells,
            value_regs,
            value_regs_top,
            value_state,
            cell_stack,
            frag_cells,
            frag_cells_base: 0,
            drops_scratch,
            value_outputs,
            value_error: None,
        })
    }

    /// Swap the double-buffered block feedback state at the end of a tick.
    pub(crate) fn swap_block_state(&mut self) {
        std::mem::swap(&mut self.block_state, &mut self.block_state_next);
        for b in &mut self.block_state_next {
            b.fill(T::ZERO);
        }
    }

    /// Allocate the persistent main λ-parameter cells at construction.
    ///
    /// Each cell is a `Void` slot owned solely by `main_cells`. Capacity is
    /// computed at build time to include these `count` slots, so allocation
    /// cannot fail in a correctly-lowered program.
    fn alloc_main_cells(arena: &mut Arena, count: usize) -> Vec<Option<crate::arena::ArenaRef>> {
        (0..count)
            .map(|_| {
                Some(
                    arena
                        .alloc(crate::arena::Value::Void)
                        .expect("main-cell allocation exceeds build-time capacity bound"),
                )
            })
            .collect()
    }

    /// Upper bound on the shared fragment capture-cell store: the worst-case
    /// call stack holds one frame per fragment (recursion is rejected), and
    /// each frame needs at most its fragment's capture cells, so the sum over
    /// all fragments bounds the deepest nesting.
    fn max_frag_cells(ir: &Ir) -> usize {
        ir.fragments
            .iter()
            .map(|f| f.num_capture_cells)
            .sum::<usize>()
    }

    /// Upper bound on the shared deferred-drop scratch: the deepest call stack
    /// queues at most one drop per value instruction of each frame, so the sum
    /// over the main track and every fragment's instructions is a strict bound.
    fn max_drops(ir: &Ir) -> usize {
        let instrs = |b: &ValueBlock| b.instrs.len();
        ir.value_blocks.iter().map(instrs).sum::<usize>()
            + ir.fragments
                .iter()
                .map(|f| f.value_blocks.iter().map(instrs).sum::<usize>())
                .sum::<usize>()
    }

    /// Release this tick's per-tick value registers.
    ///
    /// Value registers are per-tick scratch: every occupied register holds a
    /// counted arena ref that must be released before the next tick (otherwise
    /// a multi-tick value program leaks one slot per register per tick and
    /// exhausts the fixed arena). Called at the very end of a tick, after the
    /// value outputs have been read. Only the program's own registers
    /// (`0..ir.num_value_regs`) are cleared — the pre-allocated call scratch
    /// is drained by `run_fragment` on every dispatch return, so its slots are
    /// already `None`.
    pub(crate) fn clear_value_regs(&mut self) {
        debug_assert!(
            self.value_regs_top == self.ir.num_value_regs,
            "call scratch watermark not unwound at tick end"
        );
        for r in self.value_regs.iter_mut().take(self.ir.num_value_regs) {
            if let Some(r) = r.take() {
                self.arena.drop_ref(r);
            }
        }
    }

    /// Index of a named parameter, if present.
    pub fn param_index(&self, name: &str) -> Option<usize> {
        self.params_meta.iter().position(|p| p.name == name)
    }

    /// Set a parameter by index. RT-safe (plain store).
    ///
    /// For a main λ-parameter (`idx` within the leading [`Ir::num_main_cells`]
    /// cells) the value additionally lands in the persistent main cell, which
    /// is what the signal/value tracks read — so the block track sees the new
    /// value on the very next tick. The `params` store is kept in sync for
    /// readback (`param`) and for pushing dynamic built-in parameter bindings.
    pub fn set_param(&mut self, idx: usize, value: ParamValue) {
        if let Some(def) = self.params_meta.get(idx) {
            let clamped = match &value {
                ParamValue::Float(v) => {
                    ParamValue::Float((*v as f64).clamp(def.min, def.max) as f32)
                }
                ParamValue::Int(v) if *v as f64 >= def.min && (*v as f64) <= def.max => value,
                _ => value,
            };
            self.params[idx] = clamped.clone();
            if let Some(d) = self.params_dirty.get_mut(idx) {
                *d = true;
            }
            if idx < self.main_cells.len() {
                self.write_main_cell(idx, &clamped);
            }
        }
    }

    /// Store a clamped parameter value into a persistent main cell.
    ///
    /// The cell holds its value directly. The previous cell slot is released
    /// first and the fresh `Float` slot is allocated into the freed slot (the
    /// arena free-list reuses it immediately), so the cell's arena footprint
    /// stays at exactly one slot and the build-time capacity bound holds. The
    /// cell is exclusively owned by `main_cells`; this runs on the control
    /// thread, so the momentary unbound window is never observed by the signal
    /// path.
    fn write_main_cell(&mut self, cell: usize, value: &ParamValue) {
        if let Some(old) = self.main_cells[cell].take() {
            self.arena.drop_ref(old);
        }
        let v = crate::backend::interp::param_to_f64(value);
        self.main_cells[cell] = Some(
            self.arena
                .alloc(crate::arena::Value::Float(v))
                .expect("main-cell rewrite must fit the build-time capacity bound"),
        );
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

    /// Value outputs of the last processed tick, one per value output channel.
    ///
    /// Each entry is a counted arena ref (an independent owner of the slot): it
    /// survives until the next `process` tick, after which it is replaced. An
    /// empty value-output program (signal-only `main`) yields an empty slice.
    pub fn value_outputs(&self) -> &[Option<crate::arena::ArenaRef>] {
        &self.value_outputs
    }

    /// Access to the value arena, for inspecting output values (tests, tooling).
    pub fn arena(&self) -> &Arena {
        &self.arena
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

impl<T: Transcendental, const BUF: usize> Algorithm<T> for RillProgram<T, BUF> {
    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let inputs: &[&[T]] = if let Some(inp) = input { &[inp] } else { &[] };
        let mut outs: [&mut [T]; 1] = [output];
        crate::backend::interp::run_block_mimo(self, inputs, &mut outs)
    }

    fn reset(&mut self) {
        for b in &mut self.block_state {
            b.fill(T::ZERO);
        }
        for b in &mut self.block_state_next {
            b.fill(T::ZERO);
        }
        for d in &mut self.delays {
            d.clear();
        }
        for b in &mut self.builtins {
            match b {
                BuiltinInst::Block(inst) => Algorithm::reset(inst.as_mut()),
                BuiltinInst::MultichannelBlock(_) => {}
            }
        }
        // Release value-track state. Each occupied slot holds a counted arena
        // ref; dropping it (rather than just clearing the `Option`) keeps the
        // arena's fixed capacity consistent across mid-lifetime resets, so a
        // reset cannot exhaust the arena on a later tick. Slots are set to
        // `None` afterwards; the per-tick value-track executor (next task)
        // allocates fresh values on the next tick.
        for v in &mut self.value_state {
            if let Some(r) = v {
                self.arena.drop_ref(*r);
            }
            *v = None;
        }
        for r in &mut self.value_regs {
            if let Some(r) = r {
                self.arena.drop_ref(*r);
            }
            *r = None;
        }
        for r in &mut self.value_outputs {
            if let Some(r) = r {
                self.arena.drop_ref(*r);
            }
            *r = None;
        }
        for frame in &mut self.cell_stack {
            for (_, r) in frame {
                self.arena.drop_ref(*r);
            }
        }
        self.cell_stack.clear();
        self.value_regs_top = self.ir.num_value_regs;
        // The shared fragment-capture cells and deferred-drop scratch are
        // drained by every dispatch, but reset must still empty them (a reset
        // may be issued mid-dispatch is impossible — the signal thread is the
        // only dispatcher — yet a defensive clear keeps the RC accounting
        // sound if a future control-path dispatch ever appears).
        for c in self.frag_cells.drain(..) {
            self.arena.drop_ref(c);
        }
        self.frag_cells_base = 0;
        for r in self.drops_scratch.drain(..) {
            self.arena.drop_ref(r);
        }
    }
}

impl<T: Transcendental, const BUF: usize> MultichannelAlgorithm<T> for RillProgram<T, BUF> {
    fn num_inputs(&self) -> usize {
        self.ir.num_inputs
    }

    fn num_outputs(&self) -> usize {
        self.ir.num_outputs
    }

    fn process(&mut self, inputs: &[&[T]], outputs: &mut [&mut [T]]) -> ProcessResult<()> {
        crate::backend::interp::run_block_mimo(self, inputs, outputs)
    }

    fn reset(&mut self) {
        Algorithm::reset(self);
    }
}

impl<T: Transcendental, const BUF: usize> BlockBuiltin<T> for RillProgram<T, BUF> {
    fn set_param(&mut self, index: usize, value: &ParamValue) {
        self.set_param(index, value.clone());
    }
}

#[cfg(test)]
mod program_value_tests {
    use super::*;
    use crate::ir::{StateLayout, ValueLayout};

    #[test]
    fn new_program_has_empty_value_state() {
        let ir = Ir {
            instrs: Vec::new(),
            num_regs: 0,
            output_regs: Vec::new(),
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            num_main_cells: 0,
            value_blocks: Vec::new(),
            value_entry: 0,
            num_value_regs: 0,
            value_output_regs: Vec::new(),
            value_funcs: Vec::new(),
            fragments: Vec::new(),
            max_call_regs: 0,
            value_state: ValueLayout {
                capacity: 4,
                buffer_budget: 0,
                value_state_slots: 2,
            },
        };
        let prog = RillProgram::<f32, 256>::new(ir);
        assert_eq!(prog.arena.capacity(), 4);
        assert_eq!(prog.value_state.len(), 2);
    }

    #[test]
    fn reset_drops_value_track_refs() {
        let ir = Ir {
            instrs: Vec::new(),
            num_regs: 0,
            output_regs: Vec::new(),
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            num_main_cells: 0,
            value_blocks: Vec::new(),
            value_entry: 0,
            num_value_regs: 0,
            value_output_regs: Vec::new(),
            value_funcs: Vec::new(),
            fragments: Vec::new(),
            max_call_regs: 0,
            value_state: ValueLayout {
                capacity: 4,
                buffer_budget: 0,
                value_state_slots: 1,
            },
        };
        let mut prog = RillProgram::<f32, 256>::new(ir);
        // Three counted refs to the same value: one per value-state slot,
        // value register, and cell-stack frame.
        let r = prog.arena.alloc(crate::arena::Value::Int(1)).unwrap();
        prog.value_state[0] = Some(r);
        let r2 = prog.arena.copy(r).unwrap();
        prog.value_regs.push(Some(r2));
        let r3 = prog.arena.copy(r).unwrap();
        prog.cell_stack.push(vec![(0, r3)]);
        Algorithm::reset(&mut prog);
        assert_eq!(prog.value_state[0], None);
        assert_eq!(prog.value_regs[0], None);
        assert_eq!(prog.cell_stack.len(), 0);
        assert_eq!(prog.arena.rc(r), 0, "all refs dropped, slot freed");
    }
}
