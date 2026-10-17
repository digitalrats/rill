//! `RillProgram<T, BUF>` — a compiled rill-lang program.
//!
//! It owns its IR, schedule, and pre-allocated state. The program is executed
//! either by [`run_value`](RillProgram::run_value) (one whole-buffer value
//! tick, the retained pure-value path) or by the
//! [`ProgramRunner`](crate::program_runner::ProgramRunner) (which runs
//! `main : IO ()` to completion). Neither path allocates after warm-up.

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
        /// [`RillProgram::inject_stdin`](crate::program::RillProgram)).
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

/// The runtime realization of `realWorld`: the memory-stream table and the
/// captured stdio console.
///
/// `main : IO ()` threads the opaque `World` token for ordering only; the state
/// the effects actually mutate lives here, on the program. The runner captures
/// the console (`stdout`/`stderr`) so tests and drivers can assert on it, and
/// injects the console input (`stdin`).
pub struct WorldState {
    /// The memory-stream table. A stream handle's payload indexes this table
    /// (offset by [`STREAM_HANDLE_TAG`]); slots 0, 1 and 2 are
    /// `stdout`/`stderr`/`stdin`.
    pub(crate) streams: Vec<StreamHandle>,
    /// Lines written to `stdout`, in program order.
    pub stdout: Vec<String>,
    /// Lines written to `stderr`, in program order.
    pub stderr: Vec<String>,
}

impl WorldState {
    /// A fresh world: an empty memory-stream table, the two stdio sinks, and an
    /// empty console input.
    ///
    /// The table order fixes the stdio handle slots: 0 = `stdout`
    /// ([`STDOUT_HANDLE`]), 1 = `stderr` ([`STDERR_HANDLE`]), 2 = `stdin`
    /// ([`STDIN_HANDLE`]).
    pub(crate) fn new() -> Self {
        Self {
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
        }
    }
}

/// A runtime built-in instance, indexed directly by IR `instance` fields.
pub(crate) enum BuiltinInst<T: Transcendental> {
    /// An opaque whole-buffer built-in.
    Block(Box<dyn BlockBuiltin<T>>),
    /// A whole-buffer multi-channel built-in.
    MultichannelBlock(Box<dyn MultichannelBlockBuiltin<T>>),
}

/// A compiled program ready to run.
pub struct RillProgram<T: Transcendental, const BUF: usize> {
    pub(crate) ir: Ir,
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
    /// Current parameter values, indexed by [`Ir::params`].
    pub(crate) params: Vec<ParamValue>,
    /// Dirty flags: true when a param was changed since last push.
    pub(crate) params_dirty: Vec<bool>,
    /// Parameter metadata (name, default, range).
    pub(crate) params_meta: Vec<ParamDef>,
    /// Value arena (fixed capacity from IR).
    pub(crate) arena: Arena<Value>,
    /// Persistent main λ-parameter cells, one per [`Ir::num_main_cells`]. Each
    /// cell is a `Void` arena slot allocated ONCE at construction — the value
    /// track never rebinds them, so a `SetParameter` write survives across
    /// ticks. The block track (`Instr::ReadMainCell`) and value track
    /// (`ValueReadMainCell`) read these directly.
    pub(crate) main_cells: Vec<Option<crate::arena::ArenaRef>>,
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
    /// Value outputs of the last processed tick: one counted arena ref per
    /// value output channel, held across ticks (see [`value_outputs`](Self::value_outputs)).
    pub(crate) value_outputs: Vec<Option<crate::arena::ArenaRef>>,
    /// Runtime value-track error (capacity overflow), set by a collection op
    /// and consumed at the end of the value phase. Cleared each tick.
    pub(crate) value_error: Option<ProcessError>,
    /// Runtime ST regions, one per `runST` currently executing. `runST` pushes
    /// a region on entry and pops it on exit (regions nest LIFO); the region
    /// token threaded through the region's `ST` computation is
    /// `Value::Opaque(region id)`.
    pub(crate) regions: Vec<RegionState>,
    /// The ST cell table. An `STRef` handle is a `Value::Opaque` whose payload
    /// is an index into this table; a region owns the ids it allocated and
    /// releases them on exit, so the table is truncated back after each region
    /// and never grows across ticks.
    pub(crate) st_cells: Vec<Option<crate::arena::ArenaRef>>,
    /// The realized `realWorld`: the memory-stream table and captured console.
    /// `IO` actions reach it through this shared handle; the `World` token only
    /// orders them.
    pub(crate) world: std::rc::Rc<std::cell::RefCell<WorldState>>,
}

/// One running `runST` region: the STRef cells it allocated.
#[derive(Debug, Default)]
pub(crate) struct RegionState {
    /// Global cell-table ids allocated in this region, in allocation order.
    /// The cells themselves live in `RillProgram::st_cells`
    /// (`STRef` handles are plain global ids); the region releases them on
    /// exit.
    pub(crate) cells: Vec<u32>,
    /// `st_cells.len()` when the region was entered. Cells are always appended,
    /// and regions nest LIFO, so the table truncates back to this mark on exit.
    pub(crate) cell_base: usize,
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

impl<T: Transcendental, const BUF: usize> RillProgram<T, BUF> {
    /// Create a program from a compiled IR. Allocates state, delays, registers,
    /// and builds the execution schedule. Built-ins are NOT instantiated — use
    /// [`new_with`](Self::new_with) if the IR references built-in functions.
    pub fn new(ir: Ir) -> Self {
        let block_state = vec![FixedBuffer::new(); ir.state.block_state_slots];
        let block_state_next = vec![FixedBuffer::new(); ir.state.block_state_slots];
        let (delays, _allocator) = Self::alloc_delays(&ir);
        let buffers = Self::alloc_buffers(&ir);
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
            block_state,
            block_state_next,
            delays,
            buffers,
            block_len: 0,
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
            regions: Vec::new(),
            st_cells: Vec::new(),
            world: std::rc::Rc::new(std::cell::RefCell::new(WorldState::new())),
        }
    }

    /// Create a program from a compiled [`Ir`], instantiating all built-ins
    /// via the provided `Registry`. Also sets the initial `sample_rate`.
    ///
    /// Parses `builtins` from the IR, allocates registers, state, and delays,
    /// and builds the execution schedule. The resulting program implements
    /// [`Algorithm<T>`](rill_core::traits::Algorithm).
    ///
    /// When `foreign` is provided, built-in names not in `registry` resolve
    /// against the foreign registry (foreign-declared builtins).
    pub fn new_with(
        ir: Ir,
        registry: &crate::builtin::Registry<T>,
        sample_rate: f32,
        foreign: Option<&crate::ffi::ForeignRegistry<T>>,
    ) -> Result<Self, CompileError> {
        Self::build(ir, registry, sample_rate, None, foreign)
    }

    /// Create a program with a resource registry, resolving resource-backed
    /// built-ins (e.g. tape heads) from the named resources. When `foreign`
    /// is provided, non-resource built-in names not in `registry` resolve
    /// against the foreign registry.
    pub fn new_with_resources(
        ir: Ir,
        registry: &crate::builtin::Registry<T>,
        sample_rate: f32,
        resources: &mut rill_core::buffer::ResourceRegistry<T>,
        foreign: Option<&crate::ffi::ForeignRegistry<T>>,
    ) -> Result<Self, CompileError> {
        Self::build(ir, registry, sample_rate, Some(resources), foreign)
    }

    fn build(
        ir: Ir,
        registry: &crate::builtin::Registry<T>,
        sample_rate: f32,
        mut resources: Option<&mut rill_core::buffer::ResourceRegistry<T>>,
        foreign: Option<&crate::ffi::ForeignRegistry<T>>,
    ) -> Result<Self, CompileError> {
        // Allocate one shared cell per `tape_loop <capacity>` call (indexed by
        // `BuiltinInstance::tape_index`). Tape heads referencing the same index
        // get writer/reader handles over the same cell. The cells are LOCAL to
        // assembly: the `SharedWriter`/`SharedReader` handles held by the tape
        // heads are `Rc` clones into these cells and keep the buffers alive,
        // so the program itself stays `Send` (rill buffers are single-threaded
        // and must not be transferred between threads).
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
        let (delays, mut allocator) = Self::alloc_delays(&ir);
        let mut builtins = Vec::with_capacity(ir.builtins.len());
        for bi in &ir.builtins {
            let b = build_builtin(
                bi,
                registry,
                foreign,
                &mut resources,
                &tapes,
                sample_rate,
                Some(&mut allocator),
            )?;
            builtins.push(b);
        }

        let block_state = vec![FixedBuffer::new(); ir.state.block_state_slots];
        let block_state_next = vec![FixedBuffer::new(); ir.state.block_state_slots];
        let buffers = Self::alloc_buffers(&ir);
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
            block_state,
            block_state_next,
            delays,
            buffers,
            block_len: 0,
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
            regions: Vec::new(),
            st_cells: Vec::new(),
            world: std::rc::Rc::new(std::cell::RefCell::new(WorldState::new())),
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
    /// Each cell holds one of `main`'s String launch arguments and is owned
    /// solely by `main_cells`, initialized to the empty string. Capacity is
    /// computed at build time to include these `count` slots, so allocation
    /// cannot fail in a correctly-lowered program.
    fn alloc_main_cells(
        arena: &mut Arena<Value>,
        count: usize,
    ) -> Vec<Option<crate::arena::ArenaRef>> {
        (0..count)
            .map(|_| {
                Some(
                    arena
                        .alloc(crate::arena::Value::String(String::new()))
                        .expect("main-cell allocation exceeds build-time capacity bound"),
                )
            })
            .collect()
    }

    /// Carve the program-level buffer table out of a fresh region allocator:
    /// one `BUF`-length `RegionHandle<T>` per `Ir::num_buffers` (each
    /// `Value::Buffer` index). Chunk capacity is `BUF`
    /// rounded up to a power of two, so every handle fits within a single chunk
    /// (non-spanning). Memory is static with program lifetime (SP-4 Tier-1);
    /// a tick never touches the allocator.
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
    /// is drained by `run_call` on every dispatch return, so its slots are
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
    /// The `params` store is kept in sync for readback (`param`) and for
    /// pushing dynamic built-in parameter bindings. `main`'s λ-parameters are
    /// launch arguments, not parameters: bind them with
    /// [`set_main_arg`](Self::set_main_arg).
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
        }
    }

    /// Bind one of `main`'s String λ-parameters (`cell` = its position).
    ///
    /// The persistent main cell holds the String directly: the previous slot is
    /// released first and the fresh `String` slot is allocated into the freed
    /// slot (the arena free-list reuses it immediately), so the cell's footprint
    /// stays at exactly one slot and the build-time capacity bound holds. An
    /// out-of-range cell index is a no-op (the source has no such parameter).
    pub fn set_main_arg(&mut self, cell: usize, value: &str) {
        if let Some(slot) = self.main_cells.get_mut(cell) {
            if let Some(old) = slot.take() {
                self.arena.drop_ref(old);
            }
            *slot = self
                .arena
                .alloc(crate::arena::Value::String(value.to_string()))
                .ok();
        }
    }

    /// Number of `main` λ-parameters (the String launch arguments).
    pub fn num_main_args(&self) -> usize {
        self.main_cells.len()
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
        {
            let mut world = self.world.borrow_mut();
            let slot = (STDIN_HANDLE & !STREAM_HANDLE_TAG) as usize;
            if let Some(StreamHandle::Stdin {
                lines: injected,
                pos,
                pushback,
            }) = world.streams.get_mut(slot)
            {
                injected.clear();
                injected.extend(lines.iter().cloned());
                *pos = 0;
                stale.append(pushback);
            }
        }
        for r in stale {
            self.arena.drop_ref(r);
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

    /// Value outputs of the last processed tick, one per value output channel.
    ///
    /// Each entry is a counted arena ref (an independent owner of the slot): it
    /// survives until the next `process` tick, after which it is replaced. An
    /// empty value-output program (signal-only `main`) yields an empty slice.
    pub fn value_outputs(&self) -> &[Option<crate::arena::ArenaRef>] {
        &self.value_outputs
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

    /// Run one whole-buffer value tick.
    ///
    /// This is the retained pure-value execution path: it evaluates the
    /// program's value track for a full [`BUF`]-sample tick, applies the
    /// block-feedback swap, and republishes [`value_outputs`](Self::value_outputs).
    /// No external signal buffers are read or written — a whole-buffer
    /// (`Value::Buffer`) output is inspected with
    /// [`copy_output_samples`](Self::copy_output_samples).
    ///
    /// A latched value-track error (a collection/arena capacity overflow, a
    /// stream fault) is returned as a [`ProcessError`] after the tick-end
    /// cleanup, so a repeatedly failing program cannot leak the arena.
    pub fn run_value(&mut self) -> Result<(), ProcessError> {
        crate::backend::interp::run_value_tick(self)
    }

    /// Copy the samples of value output `i` into `dst` when that output is a
    /// whole-buffer value.
    ///
    /// Returns `true` when `dst` was filled (up to `dst.len()` or the tick
    /// length, whichever is smaller); `false` when the output is not a buffer
    /// (a scalar/list/record value) or `i` is out of range. Intended for tests
    /// and tooling that inspect the retained buffer path.
    pub fn copy_output_samples(&self, i: usize, dst: &mut [T]) -> bool {
        let Some(Some(r)) = self.value_outputs.get(i).copied() else {
            return false;
        };
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

    /// Reset the program to its initial state.
    ///
    /// Clears the signal state (feedback slots, delay lines), re-initialises
    /// every built-in instance, and releases the value-track state (value
    /// state slots, per-tick registers, outputs, capture cells, deferred
    /// drops) so a mid-lifetime reset cannot exhaust the fixed arena.
    pub fn reset(&mut self) {
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
            num_buffers: 0,
            input_buffers: Vec::new(),
            tapes: Vec::new(),
        };
        let prog = RillProgram::<f32, 256>::new(ir);
        assert_eq!(prog.arena.capacity(), 4);
        assert_eq!(prog.value_state.len(), 2);
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
            num_buffers: 0,
            input_buffers: Vec::new(),
            tapes: Vec::new(),
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
        prog.reset();
        assert_eq!(prog.value_state[0], None);
        assert_eq!(prog.value_regs[0], None);
        assert_eq!(prog.cell_stack.len(), 0);
        assert_eq!(prog.arena.rc(r), 0, "all refs dropped, slot freed");
    }
}
