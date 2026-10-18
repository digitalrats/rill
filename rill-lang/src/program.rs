//! `WorldState<T, BUF>` — the runtime realization of a compiled rill-lang
//! program's world.
//!
//! The compiled [`Ir`] is pure code; the world holds every piece of mutable
//! runtime state. A program is executed once to completion: its entry fragment
//! runs and the terminal register is captured into the world's `main`
//! (inspected with [`result`](WorldState::result)). The
//! [`ProgramRunner`](crate::program_runner::ProgramRunner) runs `main : IO ()`
//! through this same path. Nothing allocates after warm-up.

use std::collections::HashMap;

use crate::builtin::{BlockBuiltin, BuiltinFactoryKind, MultichannelBlockBuiltin};
use rill_core::buffer::TapeRing;
use rill_core::buffer::{FixedBuffer, SharedCell};
use rill_core::math::Transcendental;
use rill_core::memory::{BufferAllocator, RegionHandle};
use rill_core::traits::{Algorithm, ParamValue, ProcessError};

use crate::arena::{Arena, BufferPool, Value};
use crate::error::CompileError;
use crate::ir::{BuiltinInstance, Ir, ParamDef, ValueBlock};

/// Upper bound on built-in signal channels routed through one call site.
///
/// Used for stack scratch storage in the interpreter's foreign-block path.
pub(crate) const MAX_BUILTIN_CHANNELS: usize = 8;

/// The reserved parameter name behind the `?sample_rate` read.
///
/// It is **not** a named entry of [`rill_core::Params::parameters`]: the
/// dedicated [`rill_core::Params::sample_rate`] field owns the value, so a seat
/// never seeds the name and a write goes to the field.
pub(crate) const SAMPLE_RATE_PARAM: &str = "sample_rate";

/// Tag distinguishing a stream handle from an `ST` region/cell handle in the
/// shared `Value::Opaque(u32)` payload space. Stream handles occupy the high
/// half; region ids and `STRef` cell ids stay below it.
pub(crate) const STREAM_HANDLE_TAG: u32 = 1 << 31;

/// The realized `World` token (`realWorld`, the GHC `State# RealWorld` analog).
/// Only ordering matters: the mutable state it stands for lives in
/// [`WorldState`].
pub(crate) const WORLD_TOKEN: u32 = 0;

/// The realized standard-output handle (`stdout`). Stream-table slot 0.
pub(crate) const STDOUT_HANDLE: u32 = STREAM_HANDLE_TAG;

/// The realized standard-error handle (`stderr`). Stream-table slot 1.
pub(crate) const STDERR_HANDLE: u32 = STREAM_HANDLE_TAG | 1;

/// The realized standard-input handle (`stdin`). Stream-table slot 2.
pub(crate) const STDIN_HANDLE: u32 = STREAM_HANDLE_TAG | 2;

/// One entry of the runtime stream table — what an opaque stream handle
/// resolves to inside the interpreter.
pub(crate) enum StreamHandle {
    /// A memory input stream: element refs (arena-owned), a read cursor, and a
    /// pushback stack (`unread`/`atEnd`).
    Input {
        /// Element arena refs the stream owns.
        elems: Vec<crate::arena::ArenaRef>,
        /// Number of elements already consumed.
        pos: usize,
        /// Pushed-back elements, most recent last: `read` pops from here first.
        pushback: Vec<crate::arena::ArenaRef>,
    },
    /// A program-provided source (`makeInputStream`): the captured
    /// `IO (Maybe a)` action, applied to the `World` token on every `read`.
    /// `eof` latches once the action yields `Nothing`.
    Source {
        /// The captured `IO (Maybe a)` action (arena-owned).
        action: crate::arena::ArenaRef,
        /// True once the action has reported EOF.
        eof: bool,
        /// Pushed-back elements, most recent last (see [`StreamHandle::Input`]).
        pushback: Vec<crate::arena::ArenaRef>,
    },
    /// A program-provided sink (`makeOutputStream`): the captured
    /// `Maybe a -> IO ()` handler, applied to every written element. `closed`
    /// is set by `write Nothing`; later writes are inert.
    Sink {
        /// The captured `Maybe a -> IO ()` handler (arena-owned).
        handler: crate::arena::ArenaRef,
        /// True after the closing `write Nothing`.
        closed: bool,
    },
    /// The realized `stdout` sink: line-oriented text captured by the runner.
    ///
    /// `write Nothing` to the console is accepted but inert: the console is the
    /// runner's process-lifetime output handle (the program does not own it),
    /// so it is never closed. Program-owned sinks ([`StreamHandle::Sink`]) do
    /// close on `write Nothing`.
    Stdout,
    /// The realized `stderr` sink: line-oriented text captured by the runner
    /// (close semantics as for [`StreamHandle::Stdout`]).
    Stderr,
    /// The realized `stdin` source: line-oriented console text injected by the
    /// runner, read by a cursor. rill has no `ByteString`, so the element type
    /// is `String` and each line is one element.
    Stdin {
        /// The injected console lines, in order (reset by
        /// [`WorldState::inject_stdin`]).
        lines: Vec<String>,
        /// Number of lines already consumed.
        pos: usize,
        /// Pushed-back elements, most recent last: `read` pops from here first.
        pushback: Vec<crate::arena::ArenaRef>,
    },
}

impl StreamHandle {
    /// Release the arena refs this handle owns (used when a handle cannot be
    /// registered after allocation).
    pub(crate) fn drop_refs(&self, arena: &mut crate::arena::Arena<Value>) {
        match self {
            StreamHandle::Input {
                elems, pushback, ..
            } => {
                for r in elems.iter().chain(pushback.iter()) {
                    arena.drop_ref(*r);
                }
            }
            StreamHandle::Source {
                action, pushback, ..
            } => {
                arena.drop_ref(*action);
                for r in pushback {
                    arena.drop_ref(*r);
                }
            }
            StreamHandle::Sink { handler, .. } => arena.drop_ref(*handler),
            StreamHandle::Stdin { pushback, .. } => {
                for r in pushback {
                    arena.drop_ref(*r);
                }
            }
            StreamHandle::Stdout | StreamHandle::Stderr => {}
        }
    }
}

/// Caller-supplied configuration for a [`WorldState`].
///
/// The config carries **policy and external wiring only — no storage sizes**.
/// Every structural size is read from the compiled [`Ir`] at
/// [`seat`](WorldState::seat) time (the register store from
/// `ir.num_value_regs + ir.max_call_regs`, block state from
/// `ir.state.block_state_slots`, the buffer table from `ir.num_buffers`, the
/// arena from `ir.value_state.capacity`), so a world can never be sized by a
/// guess that disagrees with the program it runs.
///
/// [`Default`] carries the caller-independent policy: arena growth follows the
/// `growable-arena` crate feature, no FFI registry is attached, and the
/// parameter bag is empty.
pub struct WorldConfig<T: Transcendental> {
    /// Whether the value arena and its payload-buffer pool may append pages
    /// when a run outgrows the IR's build-time bound.
    ///
    /// `false` keeps the bound hard: an overrun latches a `ProcessError`
    /// (RT-safe). `true` is the non-RT interpreter policy (a long-running
    /// world does not fail on a program that merely exceeds its build-time
    /// estimate). The default follows the `growable-arena` crate feature, so a
    /// default-config world behaves exactly as the feature-gated build did
    /// before the policy became per-world.
    pub growable: bool,
    /// The built-in factory registry as a world-owned value.
    ///
    /// [`WorldState::seat`] resolves every `foreign`-declared built-in against
    /// this registry (an unresolvable name is a [`CompileError`] at seat). The
    /// world owns the registry, so a caller hands it over here rather than
    /// passing it to the executor on every run. `None` means "no foreign
    /// builtins": a program that declares one fails at seat.
    pub ffi: Option<crate::ffi::ForeignRegistry<T>>,
    /// The world's parameter bag (`sample_rate` + named parameters).
    ///
    /// This is the **initial state** the world is created with: the seat seeds
    /// the program's declared parameter defaults only where the bag carries no
    /// value yet, so a caller-supplied entry survives seating. The sample rate
    /// is pre-seeded on [`Params::sample_rate`](rill_core::Params::sample_rate)
    /// (default `0.0` = "not a signal program"); the seat reads the rate from
    /// this field (there is no `sample_rate` argument).
    pub params: rill_core::Params,
}

impl<T: Transcendental> Default for WorldConfig<T> {
    fn default() -> Self {
        Self {
            growable: cfg!(feature = "growable-arena"),
            ffi: None,
            params: rill_core::Params::default(),
        }
    }
}

/// The runtime realization of `realWorld`: **all** mutable runtime state.
///
/// `main : IO ()` threads the opaque `World` token for ordering only; the state
/// the effects actually mutate lives here. The world owns the memory-stream
/// table and the captured stdio console, the signal runtime (block feedback,
/// delay lines, the buffer table, built-in instances), the foreign factory
/// registry, the parameter store and its metadata, and the whole value track
/// (arena, registers, capture cells, deferred drops, ST regions). The compiled
/// [`Ir`] is pure code and lives outside the world; every executor function
/// takes `(&Ir, &mut WorldState)`. The runner captures the console
/// (`stdout`/`stderr`) so tests and drivers can assert on it, and injects the
/// console input (`stdin`).
pub struct WorldState<T: Transcendental, const BUF: usize> {
    /// The caller-supplied configuration this world was created with.
    pub(crate) config: WorldConfig<T>,
    /// The memory-stream table. A stream handle's payload indexes this table
    /// (offset by [`STREAM_HANDLE_TAG`]); slots 0, 1 and 2 are
    /// `stdout`/`stderr`/`stdin`.
    pub(crate) streams: Vec<StreamHandle>,
    /// Lines written to `stdout`, in program order.
    pub stdout: Vec<String>,
    /// Lines written to `stderr`, in program order.
    pub stderr: Vec<String>,
    /// Block-level feedback state (previous tick's whole block per slot).
    pub(crate) block_state: Vec<FixedBuffer<T, BUF>>,
    /// Current-tick feedback writes, swapped with `block_state` at tick end.
    pub(crate) block_state_next: Vec<FixedBuffer<T, BUF>>,
    /// Delay lines: region-backed ring buffers, one per `@` site.
    pub(crate) delays: Vec<RegionDelay<T>>,
    /// Program-level signal buffer table (SP-4 big-object memory): one
    /// `RegionHandle<T>` per `Value::Buffer` index, each carved at length
    /// `BUF` from a [`BufferAllocator`] at build. `Value::Buffer(i)` resolves
    /// against `buffers[i]`; mutation is in-place via `with_data_mut`.
    pub(crate) buffers: Vec<RegionHandle<T>>,
    /// Current tick's processing length (the external output slice length),
    /// stored so whole-buffer value ops operate on exactly `n` samples.
    pub(crate) block_len: usize,
    /// Runtime built-in instances (indexed by `ir.builtins` indices).
    pub(crate) builtins: Vec<BuiltinInst<T>>,
    /// The world's parameter store: `sample_rate` (the field owns `?sample_rate`)
    /// plus the named parameter map. Addressed **by name** only —
    /// [`set_param`](Self::set_param) writes here, [`param`](Self::param) reads
    /// here, and the value track resolves its IR parameter declarations to
    /// these names at run time.
    pub params: rill_core::Params,
    /// Dirty flags: true when a parameter was changed since the last push to
    /// the bound built-in. Parallel to [`Ir::params`](crate::ir::Ir::params).
    pub(crate) params_dirty: Vec<bool>,
    /// Parameter declarations (name, default, range), sourced from the IR at
    /// seat.
    pub(crate) params_meta: Vec<ParamDef>,
    /// Name → index into [`Self::params_meta`] / [`Self::params_dirty`], built
    /// at seat. The runtime store is the name-keyed [`Self::params`]; this only
    /// maps a declared name back to its IR declaration (for the range clamp and
    /// the built-in push dirt).
    pub(crate) param_names: HashMap<String, usize>,
    /// Value arena (fixed capacity from IR).
    pub(crate) arena: Arena<Value>,
    /// Per-tick value registers. The store is pre-sized to
    /// `ir.num_value_regs + ir.max_call_regs`: the leading `num_value_regs`
    /// slots are the program's own registers, the tail is the pre-allocated
    /// function-call scratch that [`run_call`](crate::backend::interp::run_call)
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
    /// [`run_call`](crate::backend::interp::run_call)). Each dispatch
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
    /// The run's terminal register: the result of executing the program's
    /// entry fragment once. Set at run end by
    /// [`exec_entry`](crate::backend::interp::exec_entry) (see
    /// [`result`](Self::result)); released at the start of the next run and by
    /// [`reset`](Self::reset).
    pub(crate) main: Option<crate::arena::ArenaRef>,
    /// Runtime value-track error (capacity overflow), set by a collection op
    /// and consumed at the end of the value phase. Cleared each tick.
    pub(crate) value_error: Option<ProcessError>,
    /// Runtime ST regions, one per `runST` currently executing. `runST` pushes
    /// a region on entry and pops it on exit (regions nest LIFO); the region
    /// token threaded through the region's `ST` computation is
    /// `Value::Opaque(region id)`.
    pub(crate) regions: Vec<RegionState>,
    /// The ST cell table. An `STRef`/`STArray` handle is a `Value::Opaque`
    /// whose payload is an index into this table; a region owns the ids it
    /// allocated and releases them on exit, so the table is truncated back
    /// after each region and never grows across ticks.
    pub(crate) st_cells: Vec<Option<StCell>>,
}

impl<T: Transcendental, const BUF: usize> WorldState<T, BUF> {
    /// A fresh world: an empty memory-stream table, the two stdio sinks, an
    /// empty console input, and empty (zero-sized) runtime state.
    ///
    /// The table order fixes the stdio handle slots: 0 = `stdout`
    /// (`STDOUT_HANDLE`), 1 = `stderr` (`STDERR_HANDLE`), 2 = `stdin`
    /// (`STDIN_HANDLE`). The IR-derived state (delays, buffers, registers,
    /// parameter store, arena) is filled in by [`seat`](Self::seat); a world
    /// built here alone is the degenerate zero-program world.
    pub fn new(config: WorldConfig<T>) -> Self {
        // The config's parameter bag is the world's initial state; the seat
        // seeds declared defaults only where it carries no value.
        let params = config.params.clone();
        Self {
            config,
            streams: vec![
                StreamHandle::Stdout,
                StreamHandle::Stderr,
                StreamHandle::Stdin {
                    lines: Vec::new(),
                    pos: 0,
                    pushback: Vec::new(),
                },
            ],
            stdout: Vec::new(),
            stderr: Vec::new(),
            block_state: Vec::new(),
            block_state_next: Vec::new(),
            delays: Vec::new(),
            buffers: Vec::new(),
            block_len: 0,
            builtins: Vec::new(),
            params,
            params_dirty: Vec::new(),
            params_meta: Vec::new(),
            param_names: HashMap::new(),
            arena: Arena::with_capacity(0),
            value_regs: Vec::new(),
            value_regs_top: 0,
            value_state: Vec::new(),
            cell_stack: Vec::new(),
            frag_cells: Vec::new(),
            frag_cells_base: 0,
            drops_scratch: Vec::new(),
            main: None,
            value_error: None,
            regions: Vec::new(),
            st_cells: Vec::new(),
        }
    }

    /// The configuration this world was created with.
    pub fn config(&self) -> &WorldConfig<T> {
        &self.config
    }

    /// Number of pre-allocated value registers: the program's own registers
    /// (`ir.num_value_regs`) plus the function-call scratch
    /// (`ir.max_call_regs`). Exposed for layout inspection (tests, tooling).
    pub fn value_regs_len(&self) -> usize {
        self.value_regs.len()
    }

    /// Number of block-feedback slots (`ir.state.block_state_slots`).
    pub fn block_state_len(&self) -> usize {
        self.block_state.len()
    }

    /// Number of program-level signal buffers (`ir.num_buffers`).
    pub fn buffers_len(&self) -> usize {
        self.buffers.len()
    }

    /// Seat this world for `ir`: assemble every structural table the program
    /// needs, resolving built-ins against the world's own
    /// [`ffi`](WorldConfig::ffi) registry.
    ///
    /// The seat is the code-meets-memory boundary. It reads **only** the IR's
    /// layout (register store, block state, buffer table, tapes, delay sites,
    /// arena capacity, parameter declarations) and applies the world's
    /// [`growable`](WorldConfig::growable) memory policy; no storage size comes
    /// from configuration. An unresolvable built-in name is a [`CompileError`]
    /// here — before any run can reach an effect.
    ///
    /// The foreign factory registry comes from [`WorldConfig::ffi`] (the world
    /// owns it); a world without one resolves no foreign built-in, so a program
    /// that declares one fails here. The sample rate comes from
    /// [`params.sample_rate`](rill_core::Params::sample_rate), which a caller
    /// pre-seeds; a non-signal world keeps the `0.0` default.
    ///
    /// Re-seating is supported: the structural tables (and the arena) are
    /// re-sized to the new IR, the value-track bookkeeping is cleared, and the
    /// console/stdout/stderr state is left untouched. Seating the same IR twice
    /// yields the same layout, but a re-seat invalidates value refs from an
    /// earlier run (`result()` returns `None` afterwards). Declared parameter
    /// **defaults** are seeded only where the world's bag has no value yet, so
    /// a pre-seeded parameter survives; `sample_rate` is never seeded from a
    /// declaration (the field owns it).
    pub fn seat(&mut self, ir: &Ir) -> Result<(), CompileError> {
        // The world's parameter store owns the rate: a caller pre-seeds
        // `params.sample_rate` before seating (there is no rate argument).
        let sample_rate = self.params.sample_rate;
        // One shared cell per `tape_loop <capacity>` call (indexed by
        // `BuiltinInstance::tape_index`). Tape heads referencing the same index
        // get writer/reader handles over the same cell. The cells are local to
        // assembly: the handles the heads hold are `Rc` clones into these
        // cells and keep the buffers alive, so the program itself stays `Send`.
        let tapes: Vec<SharedCell<T>> = ir
            .tapes
            .iter()
            .map(|&cap| {
                let tape = rill_core::buffer::TapeLoop::<T>::new(cap).ok_or_else(|| {
                    CompileError::Unsupported(format!("tape_loop capacity must be > 0 (got {cap})"))
                })?;
                Ok(SharedCell::new(Box::new(tape)))
            })
            .collect::<Result<_, CompileError>>()?;
        // Carve the delay sites first so the allocator is available to
        // built-in factories below.
        let (delays, mut allocator) = Self::alloc_delays(ir);
        // The world owns exactly one factory registry (its `ffi`), so the
        // legacy rill-core `Registry` is empty and an external resource
        // registry (`resources`) is never supplied: a name-based resource
        // binding is rejected on the source path (`compile`), and the tape path
        // resolves by index.
        let legacy = crate::builtin::Registry::<T>::new();
        let mut resources: Option<&mut rill_core::buffer::ResourceRegistry<T>> = None;
        // Build every built-in before committing anything, so a failed seat
        // leaves the previous seat's state intact.
        let mut builtins = Vec::with_capacity(ir.builtins.len());
        for bi in &ir.builtins {
            builtins.push(build_builtin(
                bi,
                &legacy,
                self.config.ffi.as_ref(),
                &mut resources,
                &tapes,
                sample_rate,
                Some(&mut allocator),
            )?);
        }
        self.delays = delays;
        self.builtins = builtins;
        self.fill_layout(ir);
        Ok(())
    }

    /// Size the IR-derived structural tables (everything the seat owns except
    /// the tapes, delay sites and built-in instances built from the
    /// allocator).
    fn fill_layout(&mut self, ir: &Ir) {
        // A seat (re)defines the whole value track. Refs from an earlier
        // seat/run belong to the arena replaced below, so the bookkeeping is
        // cleared wholesale instead of ref-by-ref.
        self.main = None;
        self.value_error = None;
        self.value_regs = vec![None; ir.num_value_regs + ir.max_call_regs];
        self.value_regs_top = ir.num_value_regs;
        self.value_state = vec![None; ir.value_state.value_state_slots];
        self.cell_stack.clear();
        self.frag_cells = Vec::with_capacity(max_frag_cells(ir));
        self.frag_cells_base = 0;
        self.drops_scratch = Vec::with_capacity(max_drops(ir));
        self.regions.clear();
        self.st_cells.clear();
        self.block_state = vec![FixedBuffer::new(); ir.state.block_state_slots];
        self.block_state_next = vec![FixedBuffer::new(); ir.state.block_state_slots];
        self.block_len = 0;
        self.buffers = Self::alloc_buffers(ir);
        // Parameter declarations come from the IR; the VALUES live in the
        // world's name-addressed bag and are seeded **absent-only**, so a
        // caller's pre-seed (world creation or `set_param` before seat) is the
        // world's initial state and survives seating.
        self.params_meta = ir.params.clone();
        self.param_names = ir
            .params
            .iter()
            .enumerate()
            .map(|(idx, def)| (def.name.clone(), idx))
            .collect();
        self.params_dirty = vec![false; ir.params.len()];
        for (idx, def) in ir.params.iter().enumerate() {
            if def.name == SAMPLE_RATE_PARAM {
                // The `sample_rate` field owns the rate: it is never a named
                // entry and never seeded from a declaration.
                continue;
            }
            match self.params.parameters.entry(def.name.clone()) {
                std::collections::hash_map::Entry::Vacant(slot) => {
                    slot.insert(ParamValue::Float(def.default as f32));
                }
                // A pre-supplied value outranks the declared default: it is an
                // explicit initial state, so it must also reach a built-in
                // bound to this parameter at the first run. The declaration is
                // only known now, so this is where the range clamp applies to a
                // value written before the seat; in-range values are unchanged.
                std::collections::hash_map::Entry::Occupied(mut slot) => {
                    let current = slot.get().clone();
                    *slot.get_mut() = clamp_param_value(def, current);
                    if let Some(dirty) = self.params_dirty.get_mut(idx) {
                        *dirty = true;
                    }
                }
            }
        }
        // The arena is sized EXACTLY from the IR's build-time capacity bound
        // (computed during lowering); the page door appends pages only when a
        // run outgrows it and the world's `growable` policy allows it.
        let mut arena = Arena::with_capacity(ir.value_state.capacity);
        arena.pool = BufferPool::new(ir.value_state.buffer_budget, self.config.growable);
        self.arena = arena;
    }

    /// Carve the program-level buffer table out of a fresh region allocator:
    /// one `BUF`-length `RegionHandle<T>` per `Ir::num_buffers` (each
    /// `Value::Buffer` index). Chunk capacity is `BUF` rounded up to a power of
    /// two, so every handle fits within a single chunk (non-spanning). Memory
    /// is static with program lifetime (SP-4 Tier-1); a tick never touches the
    /// allocator.
    fn alloc_buffers(ir: &Ir) -> Vec<RegionHandle<T>> {
        let n = ir.num_buffers;
        if n == 0 {
            return Vec::new();
        }
        let chunk_cap = BUF.max(1).next_power_of_two();
        let mut allocator = BufferAllocator::new(n * BUF, chunk_cap, T::ZERO);
        (0..n)
            .map(|_| {
                allocator
                    .allocate(BUF)
                    .expect("buffer table carve exceeds build-time budget")
            })
            .collect()
    }

    /// Carve the `@` delay sites out of a fresh region allocator.
    ///
    /// The allocator is sized from the sum of delay lengths; its chunk capacity
    /// is the largest delay length rounded up to a power of two, so every
    /// handle fits within a single chunk (non-spanning). The allocator grows
    /// on demand at build time.
    fn alloc_delays(ir: &Ir) -> (Vec<RegionDelay<T>>, BufferAllocator<T>) {
        let total: usize = ir.state.delay_lens.iter().sum();
        let max_delay = ir.state.delay_lens.iter().copied().max().unwrap_or(1);
        let chunk_cap = max_delay.max(1).next_power_of_two();
        let mut allocator = BufferAllocator::new(total.max(1), chunk_cap, T::ZERO);
        let delays = ir
            .state
            .delay_lens
            .iter()
            .map(|&l| {
                let buf = allocator.allocate(l).unwrap_or_else(|| {
                    allocator.grow(T::ZERO);
                    allocator
                        .allocate(l)
                        .expect("allocator must satisfy a delay request after growth")
                });
                RegionDelay::new(buf)
            })
            .collect();
        (delays, allocator)
    }
}

/// Upper bound on the shared fragment capture-cell store: the worst-case call
/// stack holds one frame per fragment (recursion is rejected), and each frame
/// needs at most its fragment's capture cells, so the sum over all fragments
/// bounds the deepest nesting.
fn max_frag_cells(ir: &Ir) -> usize {
    ir.fragments
        .iter()
        .map(|f| f.num_capture_cells)
        .sum::<usize>()
}

/// Clamp a parameter value against its declared range.
///
/// `Float` is clamped directly. `Int` is clamped in `f64` and truncated back to
/// an integer, then nudged one step inside the range when a fractional bound
/// would leave it outside — so an integer value always ends up in `[min, max]`.
/// Every other variant carries no numeric range and passes through unchanged.
///
/// This is the single clamp used by both `set_param` (after the seat supplied
/// the declaration) and the seat itself (a pre-seat `set_param` could not know
/// the range, so `fill_layout` clamps the caller's initial state). An in-range
/// value is returned unchanged, so clamping never rewrites a legitimate value.
fn clamp_param_value(def: &ParamDef, value: ParamValue) -> ParamValue {
    match value {
        ParamValue::Float(v) => ParamValue::Float((v as f64).clamp(def.min, def.max) as f32),
        ParamValue::Int(v) => {
            let clamped = (v as f64).clamp(def.min, def.max);
            let mut i = clamped as i32;
            if (i as f64) < def.min {
                i = i.saturating_add(1);
            } else if (i as f64) > def.max {
                i = i.saturating_sub(1);
            }
            ParamValue::Int(i)
        }
        other => other,
    }
}

/// Upper bound on the shared deferred-drop scratch: the deepest call stack
/// queues at most one drop per value instruction of each frame, so the sum over
/// the main track and every fragment's instructions is a strict bound.
fn max_drops(ir: &Ir) -> usize {
    let instrs = |b: &ValueBlock| b.instrs.len();
    ir.value_blocks.iter().map(instrs).sum::<usize>()
        + ir.fragments
            .iter()
            .map(|f| f.value_blocks.iter().map(instrs).sum::<usize>())
            .sum::<usize>()
}

impl<T: Transcendental, const BUF: usize> Default for WorldState<T, BUF> {
    fn default() -> Self {
        Self::new(WorldConfig::default())
    }
}

/// A runtime built-in instance, indexed directly by IR `instance` fields.
pub(crate) enum BuiltinInst<T: Transcendental> {
    /// An opaque whole-buffer built-in.
    Block(Box<dyn BlockBuiltin<T>>),
    /// A whole-buffer multi-channel built-in.
    MultichannelBlock(Box<dyn MultichannelBlockBuiltin<T>>),
}

/// One running `runST` region: the STRef/STArray cells it allocated.
#[derive(Debug, Default)]
pub(crate) struct RegionState {
    /// Global cell-table ids allocated in this region, in allocation order.
    /// The cells themselves live in `WorldState::st_cells`
    /// (`STRef`/`STArray` handles are plain global ids); the region releases
    /// them on exit.
    pub(crate) cells: Vec<u32>,
    /// `st_cells.len()` when the region was entered. Cells are always appended,
    /// and regions nest LIFO, so the table truncates back to this mark on exit.
    pub(crate) cell_base: usize,
}

/// One ST cell: the mutable state behind an opaque region handle.
///
/// Both variants own exactly ONE arena claim per ref they hold. A cell is
/// released by `release_st_cell` when its region exits (or when a `writeSTRef`
/// overwrites it).
pub(crate) enum StCell {
    /// An `STRef s a` cell: the current value.
    Ref(crate::arena::ArenaRef),
    /// An `STArray s i e` cell: the stored bounds (a `Pair i i` value) and the
    /// flat elements in `lo..=hi` order. `runSTArray` freezes the pair into an
    /// immutable `Array` by transferring these claims.
    Array {
        /// Stored bounds as an ordinary value (`Pair i i`).
        bounds: crate::arena::ArenaRef,
        /// Flat elements, one claim each.
        elems: Vec<crate::arena::ArenaRef>,
    },
}

/// A region-backed `@` delay site: a [`TapeRing`] over a [`RegionHandle`].
/// The ring's usable length equals the site's compile-time delay length; the
/// backing memory is carved from the program's [`BufferAllocator`] at build.
pub(crate) struct RegionDelay<T: Transcendental>(TapeRing<T, RegionHandle<T>>);

impl<T: Transcendental> RegionDelay<T> {
    pub(crate) fn new(buf: RegionHandle<T>) -> Self {
        Self(TapeRing::new(buf))
    }

    /// Read a whole block of delayed samples into `out` (delay 0).
    pub(crate) fn read_block(&self, out: &mut [T]) {
        self.0.read_block(0, out);
    }

    /// Write a whole block into the ring buffer and advance the head.
    pub(crate) fn write_block(&mut self, input: &[T]) {
        self.0.write_block(input);
    }

    /// Zero the ring and reset the head.
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

/// Build a single runtime built-in instance for `bi`, resolving the factory
/// from the legacy rill-core `Registry` or the FFI `ForeignRegistry`.
///
/// The runtime `BuiltinInst` variant is selected by the registered FACTORY KIND,
/// not by the signal arity: a `Block` factory (legacy or FFI) builds the
/// `Block` variant even for a multi-channel arity — the interpreter's
/// interleaved `Block` path serves those channels. Selecting by arity instead
/// routed a multi-channel `Block` factory into `build_multichannel_block`,
/// which has no matching variant and `.expect` panics.
fn build_builtin<T: Transcendental>(
    bi: &BuiltinInstance,
    registry: &crate::builtin::Registry<T>,
    foreign: Option<&crate::ffi::ForeignRegistry<T>>,
    resources: &mut Option<&mut rill_core::buffer::ResourceRegistry<T>>,
    tapes: &[SharedCell<T>],
    sample_rate: f32,
    allocator: Option<&mut BufferAllocator<T>>,
) -> Result<BuiltinInst<T>, CompileError> {
    let legacy_kind = registry.kind(&bi.name);
    let ffi_kind = foreign.and_then(|f| f.kind(&bi.name));
    let kind = legacy_kind.or(ffi_kind);
    // A resource-backed factory resolves its named resource through the
    // resource branch, as does any built-in carrying a resource binding.
    let is_resource = legacy_kind.is_some_and(|k| {
        matches!(
            k,
            BuiltinFactoryKind::ResourceBlock | BuiltinFactoryKind::ResourceMultichannelBlock
        )
    }) || bi.resource.is_some();
    // An FFI resource factory resolves its shared cell by tape index (the new
    // `tape_loop` path) instead of a name against an external registry.
    let ffi_is_resource = bi.tape_index.is_some()
        || ffi_kind.is_some_and(|k| {
            matches!(
                k,
                BuiltinFactoryKind::ResourceBlock | BuiltinFactoryKind::ResourceMultichannelBlock
            )
        });
    let resolve_tape = |bi: &BuiltinInstance| -> Result<
        (
            rill_core::buffer::SharedWriter<T>,
            rill_core::buffer::SharedReader<T>,
        ),
        CompileError,
    > {
        // A name-based resource binding (`bi.resource = Some(name)`) with no
        // external registry means the source referenced a tape name that was
        // never declared (`tape = tape_loop <capacity>`) and no registry was
        // supplied — report the real cause instead of the index-based
        // fallthrough error.
        if let Some(name) = &bi.resource {
            if resources.is_none() {
                return Err(CompileError::Unsupported(format!(
                    "built-in '{}' references undeclared resource '{}' (declare it with `tape = tape_loop <capacity>` or pass a registry)",
                    bi.name, name
                )));
            }
        }
        let idx = bi.tape_index.ok_or_else(|| {
            CompileError::Unsupported(format!(
                "built-in '{}' is registered as resource-backed but carries no tape index",
                bi.name
            ))
        })?;
        let cell = tapes.get(idx).ok_or_else(|| {
            CompileError::Unsupported(format!(
                "built-in '{}' references tape index {idx}, but only {} tape(s) are declared",
                bi.name,
                tapes.len()
            ))
        })?;
        Ok((cell.writer(), cell.reader()))
    };
    match kind {
        Some(
            BuiltinFactoryKind::MultichannelBlock | BuiltinFactoryKind::ResourceMultichannelBlock,
        ) => {
            let mut b: Box<dyn MultichannelBlockBuiltin<T>> = if let Some(entry) =
                registry.get(&bi.name)
            {
                if is_resource {
                    let res = bi.resource.as_deref().ok_or_else(|| {
                        CompileError::Unsupported(format!(
                            "built-in '{}' requires a resource name",
                            bi.name
                        ))
                    })?;
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
                            allocator,
                        )
                        .ok_or_else(|| {
                            CompileError::Unsupported(format!(
                                "resource built-in '{}' is not registered as resource-backed",
                                bi.name
                            ))
                        })?
                } else {
                    entry
                        .build_multichannel_block(bi.signal_ins, &bi.params, sample_rate, allocator)
                        .expect("registry build_multichannel_block failed")
                }
            } else if let Some(ffi) = foreign {
                if ffi_is_resource {
                    let (w, r) = resolve_tape(bi)?;
                    ffi.build_resource_multichannel_block(
                        &bi.name,
                        bi.signal_ins,
                        &bi.params,
                        sample_rate,
                        w,
                        r,
                        allocator,
                    )
                    .ok_or_else(|| {
                        CompileError::Unsupported(format!(
                            "foreign resource built-in '{}' is not registered",
                            bi.name
                        ))
                    })?
                } else {
                    ffi.build_multichannel_block(
                        &bi.name,
                        bi.signal_ins,
                        &bi.params,
                        sample_rate,
                        allocator,
                    )
                    .ok_or_else(|| {
                        CompileError::Unsupported(format!(
                            "foreign built-in '{}' is not registered",
                            bi.name
                        ))
                    })?
                }
            } else {
                return Err(CompileError::Unsupported(format!(
                    "unknown built-in '{}'",
                    bi.name
                )));
            };
            b.reset();
            Ok(BuiltinInst::MultichannelBlock(b))
        }
        Some(BuiltinFactoryKind::Block | BuiltinFactoryKind::ResourceBlock) | None => {
            let mut b: Box<dyn BlockBuiltin<T>> = if let Some(entry) = registry.get(&bi.name) {
                if is_resource {
                    let res = bi.resource.as_deref().ok_or_else(|| {
                        CompileError::Unsupported(format!(
                            "built-in '{}' requires a resource name",
                            bi.name
                        ))
                    })?;
                    let reg = resources.as_deref_mut().ok_or_else(|| {
                        CompileError::Unsupported(format!(
                            "built-in '{}' requires a resource registry",
                            bi.name
                        ))
                    })?;
                    entry
                        .build_resource_block(&bi.params, sample_rate, reg, res, allocator)
                        .ok_or_else(|| {
                            CompileError::Unsupported(format!(
                                "resource built-in '{}' is not registered as resource-backed",
                                bi.name
                            ))
                        })?
                } else {
                    entry
                        .build_block(&bi.params, sample_rate, allocator)
                        .expect("registry build_block failed for block builtin")
                }
            } else if let Some(ffi) = foreign {
                if ffi_is_resource {
                    let (w, r) = resolve_tape(bi)?;
                    ffi.build_resource_block(&bi.name, &bi.params, sample_rate, w, r, allocator)
                        .ok_or_else(|| {
                            CompileError::Unsupported(format!(
                                "foreign resource built-in '{}' is not registered",
                                bi.name
                            ))
                        })?
                } else {
                    ffi.build_block(&bi.name, &bi.params, sample_rate, allocator)
                        .ok_or_else(|| {
                            CompileError::Unsupported(format!(
                                "foreign built-in '{}' is not registered",
                                bi.name
                            ))
                        })?
                }
            } else {
                return Err(CompileError::Unsupported(format!(
                    "unknown built-in '{}'",
                    bi.name
                )));
            };
            Algorithm::init(b.as_mut(), sample_rate);
            Ok(BuiltinInst::Block(b))
        }
    }
}

impl<T: Transcendental, const BUF: usize> WorldState<T, BUF> {
    /// Swap the double-buffered block feedback state at the end of a tick.
    pub(crate) fn swap_block_state(&mut self) {
        std::mem::swap(&mut self.block_state, &mut self.block_state_next);
        for b in &mut self.block_state_next {
            b.fill(T::ZERO);
        }
    }

    /// Release this run's per-run value registers.
    ///
    /// Value registers are per-run scratch: every occupied register holds a
    /// counted arena ref that must be released before the next run (otherwise
    /// a repeated-run value program leaks one slot per register per run and
    /// exhausts the fixed arena). Called at the very end of a run, after the
    /// result has been captured into `main`. Only the program's own registers
    /// (`0..num_value_regs`) are cleared — the pre-allocated call scratch is
    /// drained by `run_call` on every dispatch return, so its slots are
    /// already `None`.
    pub(crate) fn clear_value_regs(&mut self, num_value_regs: usize) {
        debug_assert!(
            self.value_regs_top == num_value_regs,
            "call scratch watermark not unwound at run end"
        );
        for r in self.value_regs.iter_mut().take(num_value_regs) {
            if let Some(r) = r.take() {
                self.arena.drop_ref(r);
            }
        }
    }

    /// Set a parameter by name. RT-safe for a declared name: an in-place map
    /// write plus a dirty flag.
    ///
    /// The value is clamped against the parameter's seated declaration range
    /// (`Float` and `Int` alike); an undeclared name is stored as-is.
    /// `"sample_rate"` is the world's
    /// [`Params::sample_rate`](rill_core::Params::sample_rate) field, not a
    /// named map entry.
    ///
    /// **Allocation:** the only allocating case is an *undeclared* name, whose
    /// key is not in the map yet (it needs a fresh `String` and may grow the
    /// map). Every declared name was pre-seeded by the seat, so the live
    /// parameter path — which only ever addresses declared names — rewrites the
    /// existing entry in place.
    pub fn set_param(&mut self, name: &str, value: ParamValue) {
        if name == SAMPLE_RATE_PARAM {
            let rate = match &value {
                ParamValue::Float(v) => *v as f64,
                ParamValue::Int(v) => *v as f64,
                _ => return,
            };
            self.params.sample_rate = rate as f32;
            if let Some(&idx) = self.param_names.get(name) {
                if let Some(dirty) = self.params_dirty.get_mut(idx) {
                    *dirty = true;
                }
            }
            return;
        }
        let clamped = match self
            .param_names
            .get(name)
            .and_then(|&i| self.params_meta.get(i))
        {
            Some(def) => clamp_param_value(def, value),
            None => value,
        };
        match self.params.parameters.get_mut(name) {
            Some(slot) => *slot = clamped,
            // Every declared name is seeded by `fill_layout`, so a miss is
            // necessarily an undeclared name: insert it (the allocating case).
            None => {
                self.params.parameters.insert(name.to_string(), clamped);
            }
        }
        if let Some(&idx) = self.param_names.get(name) {
            if let Some(dirty) = self.params_dirty.get_mut(idx) {
                *dirty = true;
            }
        }
    }

    /// Current value of a parameter by name.
    ///
    /// `"sample_rate"` always resolves to the world's rate field; every other
    /// name is looked up in the named map, so an undeclared name is `None`.
    pub fn param(&self, name: &str) -> Option<ParamValue> {
        if name == SAMPLE_RATE_PARAM {
            return Some(ParamValue::Float(self.params.sample_rate));
        }
        self.params.parameters.get(name).cloned()
    }

    /// The runtime value of a parameter declared in the IR, resolved **by
    /// name**: `"sample_rate"` reads [`Params::sample_rate`], everything else
    /// the named map (an absent entry reads as `Float(0.0)`, the historical
    /// unset-parameter default).
    pub(crate) fn param_value(&self, name: &str) -> ParamValue {
        if name == SAMPLE_RATE_PARAM {
            return ParamValue::Float(self.params.sample_rate);
        }
        self.params
            .parameters
            .get(name)
            .cloned()
            .unwrap_or(ParamValue::Float(0.0))
    }

    /// Metadata for all parameters (name, default, range).
    pub fn params_meta(&self) -> &[ParamDef] {
        &self.params_meta
    }

    /// Replace the console input (`stdin`) lines and rewind the stream cursor.
    ///
    /// The runner's captured console realizes the program's stdio, so the
    /// caller supplies the finite, line-oriented input that `read stdin`
    /// yields; after the lines are drained the stream reports EOF. Pushback
    /// entries left over from an earlier run are released (the stream keeps its
    /// `unread` stack across runs otherwise).
    pub(crate) fn inject_stdin(&mut self, lines: &[String]) {
        let mut stale = Vec::new();
        let slot = (STDIN_HANDLE & !STREAM_HANDLE_TAG) as usize;
        if let Some(StreamHandle::Stdin {
            lines: injected,
            pos,
            pushback,
        }) = self.streams.get_mut(slot)
        {
            injected.clear();
            injected.extend(lines.iter().cloned());
            *pos = 0;
            stale.append(pushback);
        }
        for r in stale {
            self.arena.drop_ref(r);
        }
    }

    /// The result of the program's single run: the terminal arena ref captured
    /// by [`exec_entry`](crate::backend::interp::exec_entry) into `main`.
    ///
    /// A rill-lang program executes its entry fragment exactly ONCE, so this is
    /// the program's one value-level result — the whole-buffer
    /// (`Value::Buffer`) forms are inspected with
    /// [`copy_buffer`](Self::copy_buffer). Returns `None` before the first run
    /// or when the run produced no output register.
    pub fn result(&self) -> Option<crate::arena::ArenaRef> {
        self.main
    }

    /// Access to the value arena, for inspecting output values (tests, tooling).
    pub fn arena(&self) -> &Arena<Value> {
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

    /// Copy the samples of a whole-buffer run result into `dst`.
    ///
    /// Returns `true` when `dst` was filled (up to `dst.len()` or the run
    /// length, whichever is smaller); `false` when `r` is not a buffer value
    /// (a scalar/list/record value) or its buffer is gone. Intended for tests
    /// and tooling that inspect the retained buffer path.
    pub fn copy_buffer(&self, r: crate::arena::ArenaRef, dst: &mut [T]) -> bool {
        let Some(Value::Buffer(idx)) = self.arena.get(r) else {
            return false;
        };
        let Some(buf) = self.buffers.get(*idx) else {
            return false;
        };
        let n = dst.len().min(self.block_len);
        buf.with_data(|b| dst[..n].copy_from_slice(&b[..n]));
        true
    }

    /// Reset the world to its initial state.
    ///
    /// Clears the signal state (feedback slots, delay lines), re-initialises
    /// every built-in instance, and releases the value-track state (value
    /// state slots, per-tick registers, outputs, capture cells, deferred
    /// drops) so a mid-lifetime reset cannot exhaust the fixed arena.
    ///
    /// `num_value_regs` is the IR's register count: it is the watermark the
    /// per-tick register store returns to after the value-track state is
    /// released.
    pub fn reset(&mut self, num_value_regs: usize) {
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
        // reset cannot exhaust the arena on a later tick.
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
        if let Some(r) = self.main.take() {
            self.arena.drop_ref(r);
        }
        for frame in &mut self.cell_stack {
            for (_, r) in frame {
                self.arena.drop_ref(*r);
            }
        }
        self.cell_stack.clear();
        self.value_regs_top = num_value_regs;
        // The shared fragment-capture cells and deferred-drop scratch are
        // drained by every dispatch, but reset must still empty them (a reset
        // mid-dispatch is impossible — a single dispatcher runs the track — yet
        // a defensive clear keeps the RC accounting sound if a future
        // control-path dispatch ever appears).
        for c in self.frag_cells.drain(..) {
            self.arena.drop_ref(c);
        }
        self.frag_cells_base = 0;
        for r in self.drops_scratch.drain(..) {
            self.arena.drop_ref(r);
        }
    }
}

#[cfg(test)]
mod program_value_tests {
    use super::*;
    use crate::ir::{StateLayout, ValueLayout};

    #[test]
    fn new_program_has_empty_value_state() {
        let ir = Ir {
            main_io_unit: false,
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            value_blocks: Vec::new(),
            value_entry: 0,
            num_value_regs: 0,
            result_reg: 0,
            value_funcs: Vec::new(),
            fragments: Vec::new(),
            max_call_regs: 0,
            value_state: ValueLayout {
                capacity: 4,
                buffer_budget: 0,
                value_state_slots: 2,
            },
            num_buffers: 0,
            input_buffers: Vec::new(),
            tapes: Vec::new(),
        };
        let mut world = WorldState::<f32, 256>::new(WorldConfig::default());
        world.seat(&ir).expect("seat");
        assert_eq!(world.arena.capacity(), 4);
        assert_eq!(world.value_state.len(), 2);
    }

    #[test]
    fn seat_marks_a_preseeded_param_pending_for_the_builtin_push() {
        // A value supplied BEFORE the seat must reach a built-in bound to that
        // parameter: the built-in is constructed from the IR defaults, so the
        // seat marks every pre-supplied parameter dirty and the next run pushes
        // it (`push_builtin_params`). Without this, a pre-seeded world would
        // read the new value through `?name` but the bound built-in would keep
        // its constructed default.
        let ir = Ir {
            main_io_unit: false,
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: vec![ParamDef {
                name: "level".to_string(),
                default: 0.0,
                min: f64::NEG_INFINITY,
                max: f64::INFINITY,
            }],
            value_blocks: Vec::new(),
            value_entry: 0,
            num_value_regs: 0,
            result_reg: 0,
            value_funcs: Vec::new(),
            fragments: Vec::new(),
            max_call_regs: 0,
            value_state: ValueLayout {
                capacity: 4,
                buffer_budget: 0,
                value_state_slots: 0,
            },
            num_buffers: 0,
            input_buffers: Vec::new(),
            tapes: Vec::new(),
        };
        let mut preseeded = WorldState::<f32, 256>::new(WorldConfig {
            params: rill_core::Params::new(44_100.0),
            ..WorldConfig::default()
        });
        preseeded.set_param("level", ParamValue::Float(0.5));
        preseeded.seat(&ir).expect("seat");
        assert_eq!(preseeded.param("level"), Some(ParamValue::Float(0.5)));
        assert!(
            preseeded.params_dirty[0],
            "a pre-supplied value is an initial state and must be pushed"
        );

        let mut defaulted = WorldState::<f32, 256>::new(WorldConfig {
            params: rill_core::Params::new(44_100.0),
            ..WorldConfig::default()
        });
        defaulted.seat(&ir).expect("seat");
        assert_eq!(defaulted.param("level"), Some(ParamValue::Float(0.0)));
        assert!(
            !defaulted.params_dirty[0],
            "a seeded default already matches the built-in's construction"
        );
        assert_eq!(
            defaulted.params.sample_rate, 44_100.0,
            "the world's pre-seeded rate survives the seat"
        );
    }

    #[test]
    fn set_param_clamps_to_the_declared_range() {
        let ir = Ir {
            main_io_unit: false,
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: vec![ParamDef {
                name: "cutoff".to_string(),
                default: 1.0,
                min: 0.0,
                max: 2.0,
            }],
            value_blocks: Vec::new(),
            value_entry: 0,
            num_value_regs: 0,
            result_reg: 0,
            value_funcs: Vec::new(),
            fragments: Vec::new(),
            max_call_regs: 0,
            value_state: ValueLayout {
                capacity: 4,
                buffer_budget: 0,
                value_state_slots: 0,
            },
            num_buffers: 0,
            input_buffers: Vec::new(),
            tapes: Vec::new(),
        };
        let mut world = WorldState::<f32, 256>::new(WorldConfig::default());
        world.seat(&ir).expect("seat");

        world.set_param("cutoff", ParamValue::Float(9.0));
        assert_eq!(world.param("cutoff"), Some(ParamValue::Float(2.0)));
        world.set_param("cutoff", ParamValue::Float(-9.0));
        assert_eq!(world.param("cutoff"), Some(ParamValue::Float(0.0)));
        // Integers are clamped too: an out-of-range `Int` must not slip past
        // the range, while an in-range one keeps its variant and value.
        world.set_param("cutoff", ParamValue::Int(9));
        assert_eq!(world.param("cutoff"), Some(ParamValue::Int(2)));
        world.set_param("cutoff", ParamValue::Int(-9));
        assert_eq!(world.param("cutoff"), Some(ParamValue::Int(0)));
        world.set_param("cutoff", ParamValue::Int(1));
        assert_eq!(world.param("cutoff"), Some(ParamValue::Int(1)));
        // An undeclared name is stored as-is (no declaration to clamp against).
        world.set_param("other", ParamValue::Float(9.0));
        assert_eq!(world.param("other"), Some(ParamValue::Float(9.0)));
    }

    #[test]
    fn seat_clamps_a_preseeded_param_to_the_declared_range() {
        // A value supplied before the seat cannot be clamped — the declaration
        // is not known yet — so `fill_layout` applies the clamp when it meets
        // the caller's entry. An in-range pre-seed is the identity and stays
        // untouched; an out-of-range one is pulled into the declared range.
        let ir = Ir {
            main_io_unit: false,
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: vec![ParamDef {
                name: "level".to_string(),
                default: 1.0,
                min: 0.0,
                max: 2.0,
            }],
            value_blocks: Vec::new(),
            value_entry: 0,
            num_value_regs: 0,
            result_reg: 0,
            value_funcs: Vec::new(),
            fragments: Vec::new(),
            max_call_regs: 0,
            value_state: ValueLayout {
                capacity: 4,
                buffer_budget: 0,
                value_state_slots: 0,
            },
            num_buffers: 0,
            input_buffers: Vec::new(),
            tapes: Vec::new(),
        };
        let mut out_of_range = WorldState::<f32, 256>::new(WorldConfig::default());
        out_of_range.set_param("level", ParamValue::Float(9.0));
        out_of_range.seat(&ir).expect("seat");
        assert_eq!(
            out_of_range.param("level"),
            Some(ParamValue::Float(2.0)),
            "the seated declaration range applies to a pre-seat value"
        );
        assert!(
            out_of_range.params_dirty[0],
            "a clamped pre-supplied value is still an initial state and must be pushed"
        );

        let mut in_range = WorldState::<f32, 256>::new(WorldConfig::default());
        in_range.set_param("level", ParamValue::Float(0.25));
        in_range.seat(&ir).expect("seat");
        assert_eq!(
            in_range.param("level"),
            Some(ParamValue::Float(0.25)),
            "clamping an in-range pre-seed is the identity"
        );
    }

    #[test]
    fn reset_drops_value_track_refs() {
        let ir = Ir {
            main_io_unit: false,
            num_inputs: 0,
            num_outputs: 0,
            state: StateLayout::default(),
            builtins: Vec::new(),
            params: Vec::new(),
            value_blocks: Vec::new(),
            value_entry: 0,
            num_value_regs: 0,
            result_reg: 0,
            value_funcs: Vec::new(),
            fragments: Vec::new(),
            max_call_regs: 0,
            value_state: ValueLayout {
                capacity: 4,
                buffer_budget: 0,
                value_state_slots: 1,
            },
            num_buffers: 0,
            input_buffers: Vec::new(),
            tapes: Vec::new(),
        };
        let mut world = WorldState::<f32, 256>::new(WorldConfig::default());
        world.seat(&ir).expect("seat");
        // Three counted refs to the same value: one per value-state slot,
        // value register, and cell-stack frame.
        let r = world.arena.alloc(crate::arena::Value::Int(1)).unwrap();
        world.value_state[0] = Some(r);
        let r2 = world.arena.copy(r).unwrap();
        world.value_regs.push(Some(r2));
        let r3 = world.arena.copy(r).unwrap();
        world.cell_stack.push(vec![(0, r3)]);
        world.reset(ir.num_value_regs);
        assert_eq!(world.value_state[0], None);
        assert_eq!(world.value_regs[0], None);
        assert_eq!(world.cell_stack.len(), 0);
        assert_eq!(world.arena.rc(r), 0, "all refs dropped, slot freed");
    }
}
