//! Flat, register-machine intermediate representation.
//!
//! The IR computes the program's single output sample from its input sample(s)
//! using a scratch register file (`Vec<f64>`, cleared per sample) plus a
//! persistent state vector for feedback, `@` delays, and built-in calls.
//! Instructions are in evaluation order; each writes exactly one register (SSA-like).
//!
//! The interpreter executes this per sample. The future Cranelift backend
//! consumes the same structure.

use crate::builtin::BuiltinKind;

/// A register index into the per-sample scratch file.
pub type Reg = usize;

/// A register index into the per-tick value register file.
pub type ValueReg = usize;

/// A slot index into the persistent state vector.
pub type StateSlot = usize;

/// A unique identifier for a probe point in the IR.
#[cfg(feature = "debug")]
pub type ProbeId = u32;

/// A single unary math primitive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    /// negate
    Neg,
    /// absolute value
    Abs,
    /// sine
    Sin,
    /// cosine
    Cos,
    /// tangent
    Tan,
    /// square root
    Sqrt,
    /// e^x
    Exp,
    /// natural log
    Ln,
    /// hyperbolic tangent
    Tanh,
}

/// A single binary math primitive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinArith {
    /// +
    Add,
    /// -
    Sub,
    /// *
    Mul,
    /// /
    Div,
    /// %
    Rem,
    /// min
    Min,
    /// max
    Max,
    /// atan2 (four-quadrant arctangent, elementwise)
    Atan2,
}

/// A value-track collection operation dispatched by the interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueBuiltinOp {
    /// Prepend an element to a list.
    Cons,
    /// Build a lazy cons cell (`Value::Cons`): the tail is NOT forced — this
    /// is the constructor behind `:` in complete mode and enables infinite
    /// sequences.
    LazyCons,
    /// The empty lazy list (`Value::Nil`).
    Nil,
    /// Read the first element of a list (a `Maybe`).
    Head,
    /// Drop the first element of a list.
    Tail,
    /// Count the elements of a list/set/map (an immutable array reports its
    /// flat element count — see `arrayElems`).
    Length,
    /// Map a function over a list.
    Map,
    /// Left-fold a function over a list.
    Fold,
    /// Keep the elements of a list satisfying a predicate.
    Filter,
    /// Allocate an empty list with a capacity.
    ListEmpty,
    /// Insert a (key, value) pair into a map.
    InsertMap,
    /// Look up a key in a map (a `Maybe`).
    Lookup,
    /// Test whether an element belongs to a map/set.
    Member,
    /// Insert an element into a set.
    InsertSet,
    /// Allocate an empty map with a capacity.
    MapEmpty,
    /// Allocate an empty set with a capacity.
    SetEmpty,
    /// Bind (concat-map) a function over a list, splicing the results
    /// (`bind xs f` for the `Monad List` instance).
    ConcatMap,
    /// Concatenate two lists (`Monoid List.mappend`).
    AppendList,
    /// Concatenate two strings (`Monoid String.mappend`).
    ConcatString,
    /// Render a Float as a String (`show_float`).
    ShowFloat,
    /// Render an Int as a String (`show_int`).
    ShowInt,
    /// Render a Bool as a String (`show_bool`): `true` → `"True"`,
    /// `false` → `"False"` (the Haskell constructor spelling).
    ShowBool,
    // ---- Immutable array storage (flat, type-agnostic primitives) ----
    // Every index computation lives in the language (`Ix`); the interpreter
    // only ever sees a stored `Pair i i`, flat element lists and flat offsets.
    /// Build an immutable array from its bounds and flat element list:
    /// `arrayFromList bounds elems`.
    ArrayFromList,
    /// Read the stored bounds (`Pair i i`) back out: `arrayBounds arr`.
    ArrayBounds,
    /// Read the flat elements back out as a `List e`: `arrayElems arr`.
    ArrayElems,
    /// Read the element at a FLAT offset (bounds-checked; an out-of-range
    /// offset latches a clean runtime fault): `arrayIndexFlat arr i`.
    ArrayIndexFlat,
    /// Return a NEW array with the given flat `(offset, value)` pairs applied:
    /// `arrayUpdate arr assocs`. The source array is never mutated.
    ArrayUpdate,
    /// Return a NEW array with a binary accumulator applied to the given flat
    /// `(offset, value)` pairs: `arrayAccum arr f assocs`.
    ArrayAccum,
    /// Copy a signal buffer into an immutable array of `Int`-indexed `Float`s
    /// (`lo = 0`, `hi = len - 1`): `arrayFromBuffer buf`.
    ArrayFromBuffer,
    /// Copy an immutable `Int`-indexed `Float` array into a signal buffer:
    /// `bufferFromArray arr`.
    BufferFromArray,
    /// The inclusive `Int` range of a bounds pair: `listRange (l, u)` →
    /// `[l, l+1, …, u]` (the empty list when `u < l`). The single
    /// list-GENERATING primitive — the language has no recursion in strict mode
    /// and no `enumFromTo`, so `Ix.range` needs it to build its result. It takes
    /// a `Pair Int Int`, so it is Int-only by construction.
    ListRange,
    /// `arrayFromList` with the STRICT length check behind the language-level
    /// `listArray`: `arrayFromListChecked bounds rangeSize elems`. A mismatch
    /// between the element count and `rangeSize` (computed by the language from
    /// the `Ix` instance) latches a clean fault instead of building a
    /// mis-indexed array.
    ArrayFromListChecked,
}

/// A value-track effect operation (the ST/region and IO-stream primitives).
///
/// The ST operations are first-class: lowered to
/// [`Value::BuiltinCallable`](crate::arena::Value::BuiltinCallable), captured
/// with their arguments and applied to the region's STState token by the
/// enclosing `ST` record's `unST` closure. `RunST`/`RunSTArray` are emitted
/// directly as a [`ValueInstr::ValueEffectOp`] — they consume an `ST` value and
/// run its region, the latter freezing the region's `STArray` result.
///
/// The IO-stream operations use the same shape: lowered to a `BuiltinCallable`
/// and applied to the `World` token by the enclosing `IO` record's `unIO`
/// closure. They read and mutate the program's
/// [`WorldState`](crate::program::WorldState), which is the runtime
/// realization of `realWorld`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueEffectOp {
    /// `runST st`: enter a fresh, isolated region; apply the `ST` value's
    /// `unST` closure to a fresh STState token, discard the final state, and
    /// release every cell the region allocated.
    RunST,
    /// `newSTRef v`: allocate a cell holding `v` in the current region.
    NewStRef,
    /// `readSTRef r`: read the current cell value.
    ReadStRef,
    /// `writeSTRef r v`: overwrite the cell value.
    WriteStRef,
    /// `modifySTRef r f`: overwrite the cell value with `f` applied to it.
    ModifyStRef,
    /// `newArrayFlat bounds n init`: allocate a mutable array cell in the
    /// current region. `bounds` is the stored `Pair i i` value, `n` the flat
    /// element count (the language computes it via `rangeSize`), and `init`
    /// the initial value of every element.
    NewArrayFlat,
    /// `readArrayFlat a i`: read the element at the flat offset `i`.
    ReadArrayFlat,
    /// `writeArrayFlat a i v`: overwrite the element at the flat offset `i`.
    WriteArrayFlat,
    /// `modifyArrayFlat a i f`: overwrite the element at the flat offset `i`
    /// with `f` applied to its current value.
    ModifyArrayFlat,
    /// `getBounds a`: read the cell's stored bounds back out as a `Pair i i`.
    GetArrayBounds,
    /// `getElems a`: read the cell's current elements back out as a `List e`.
    GetArrayElems,
    /// `newArrayFromListFlat bounds n xs`: allocate a mutable array cell in
    /// the current region and seed its elements from `xs` (the strict
    /// `newListArray` counterpart of [`Self::NewArrayFlat`]). `n` is the
    /// language-computed `rangeSize`; a list whose length differs latches a
    /// clean fault.
    NewArrayFromListFlat,
    /// `runSTArray st`: like [`Self::RunST`], but FREEZE the region's
    /// `STArray` result into an immutable `Array` (a refcount transfer — no
    /// element copies).
    RunSTArray,
    /// `fromList xs`: allocate a memory input stream over `xs` in the World's
    /// stream table and yield its opaque handle.
    StreamFromList,
    /// `toList s`: drain a memory input stream into a `List`.
    StreamToList,
    /// `write mb out`: push `Just a` into an output stream (`Nothing` closes
    /// it); the stdio targets append to the World's captured console.
    StreamWrite,
    /// `read s`: pull the next element of an input stream; `Nothing` at EOF.
    StreamRead,
    /// `atEnd s`: report whether an input stream is at EOF, without consuming
    /// the pending element.
    StreamAtEnd,
    /// `unread x s`: push `x` back onto an input stream; the next `read`
    /// returns it.
    StreamUnread,
    /// `connect input output`: pull from `input` into `output` until input EOF,
    /// then close `output` (`write Nothing`).
    StreamConnect,
    /// `makeInputStream act`: wrap the program's `IO (Maybe a)` action as a
    /// memory input stream; each `read` runs the action.
    StreamMakeInput,
    /// `makeOutputStream sink`: wrap the program's `Maybe a -> IO ()` handler
    /// as a memory output stream; each `write` applies the handler.
    StreamMakeOutput,
}

/// Value-track comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    /// Equal to.
    Eq,
    /// Not equal to.
    Ne,
    /// Less than.
    Lt,
    /// Greater than.
    Gt,
    /// Less than or equal to.
    Le,
    /// Greater than or equal to.
    Ge,
}

/// Value-track boolean logic operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicOp {
    /// Boolean conjunction.
    And,
    /// Boolean disjunction.
    Or,
}

/// A straight-line run of value instructions ending in a terminator.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueBlock {
    /// Instructions executed in order.
    pub instrs: Vec<ValueInstr>,
    /// How the block ends.
    pub term: ValueTerm,
}

impl Default for ValueBlock {
    fn default() -> Self {
        ValueBlock {
            instrs: Vec::new(),
            term: ValueTerm::Halt,
        }
    }
}

/// The data-driven successor(s) of a value block. Control flow lives in these
/// ids (data), not in the Rust call stack.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueTerm {
    /// Run the block with the given id.
    Fallthrough(usize),
    /// Branch on a Bool value register.
    Branch {
        /// Value register holding the Bool condition.
        cond: usize,
        /// Block to run when the condition is true.
        then: usize,
        /// Block to run when the condition is false.
        els: usize,
    },
    /// Branch on a sum value's constructor tag.
    BranchCtor {
        /// Value register holding the scrutinee sum.
        slot: usize,
        /// Constructor tag that selects the `then` block.
        ctor: u32,
        /// Block to run when the scrutinee's ctor matches.
        then: usize,
        /// Block to run otherwise.
        els: usize,
    },
    /// End of the value track (or fragment).
    Halt,
}

/// A per-tick value instruction. Executed once per block in the value-track
/// phase, alongside the whole-buffer block instructions.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueInstr {
    /// Push a scope frame onto the runtime cell stack.
    ValuePushScope,
    /// Pop a scope frame, releasing cell refs.
    ValuePopScope,
    /// Bind a cell (fresh slot) for a local variable.
    ValueBindCell {
        /// Destination value register (holds the cell ref).
        dst: usize,
    },
    /// Read a variable: copy the value out of the cell into a fresh slot
    /// (the result is a new owner, it does not share the cell's value).
    ///
    /// Inside a function fragment, `cell` is a CAPTURE index: a free variable
    /// of the lambda, read from the call's temporary env frame (see
    /// [`run_call`](crate::backend::interp::run_call)) when
    /// `cell < active_fragment_cells.len()`. Outside a fragment, `cell` is a
    /// value register holding a cell ref.
    ValueReadCell {
        /// Destination value register.
        dst: usize,
        /// Value register holding the cell ref, or the fragment capture index.
        cell: usize,
    },
    /// Read a main λ-parameter cell into a fresh value slot (the result is a
    /// new owner; a `Void` cell reads as `Float(0.0)`).
    ValueReadMainCell {
        /// Destination value register.
        dst: usize,
        /// Index into the persistent main-cell store.
        cell: usize,
    },
    /// Write a variable: value ref into a cell.
    ValueWriteCell {
        /// Cell register to write into.
        cell: usize,
        /// Value register to write.
        src: usize,
    },
    /// Create an Int value.
    ValueConstInt {
        /// Destination value register.
        dst: usize,
        /// The value.
        value: i64,
    },
    /// Create a Float value.
    ValueConstFloat {
        /// Destination value register.
        dst: usize,
        /// The value.
        value: f64,
    },
    /// Element-wise arithmetic on two value channels: reads the Float (or Int)
    /// payloads of the value registers `a` and `b`, allocates the Float result
    /// into `dst`. Unbound operands read as `0.0`.
    ValueAdd {
        /// Destination value register (the Float result).
        dst: usize,
        /// Left operand value register.
        a: usize,
        /// Right operand value register.
        b: usize,
    },
    /// See [`ValueInstr::ValueAdd`].
    ValueSub {
        /// Destination value register (the Float result).
        dst: usize,
        /// Left operand value register.
        a: usize,
        /// Right operand value register.
        b: usize,
    },
    /// See [`ValueInstr::ValueAdd`].
    ValueMul {
        /// Destination value register (the Float result).
        dst: usize,
        /// Left operand value register.
        a: usize,
        /// Right operand value register.
        b: usize,
    },
    /// See [`ValueInstr::ValueAdd`].
    ValueDiv {
        /// Destination value register (the Float result).
        dst: usize,
        /// Left operand value register.
        a: usize,
        /// Right operand value register.
        b: usize,
    },
    /// Construct a record: alloc + write field refs.
    ValueConstructRecord {
        /// Destination value register.
        dst: usize,
        /// Field value registers (refs).
        fields: Vec<usize>,
    },
    /// Construct a sum: alloc + write constructor + payload.
    ValueConstructSum {
        /// Destination value register.
        dst: usize,
        /// Constructor index.
        ctor: u32,
        /// Payload value registers.
        payload: Vec<usize>,
    },
    /// Project a field: read `field` of the record in `slot`.
    ValueProject {
        /// Destination value register.
        dst: usize,
        /// Record value register.
        slot: usize,
        /// Field index.
        field: usize,
    },
    /// COW-mutate a field of a record.
    ValueUpdateField {
        /// Record value register (in/out: may be COW-copied).
        slot: usize,
        /// Field index.
        field: usize,
        /// New field value register.
        src: usize,
    },
    /// Wrap a value in a newtype.
    ValueNewtype {
        /// Destination value register.
        dst: usize,
        /// Inner value register.
        src: usize,
    },
    /// Unwrap a newtype to its inner value.
    ValueUnwrap {
        /// Destination value register.
        dst: usize,
        /// Newtype value register.
        src: usize,
    },
    /// Call a first-class function value at runtime: dispatches to the
    /// [`FragmentIr`] referenced by the callable's `fragment` id, binding the
    /// caller's argument registers and copying the fragment's result into `dst`.
    ValueCallFunc {
        /// Destination value register.
        dst: usize,
        /// Value register holding the [`Value::Callable`] to call.
        closure_slot: usize,
        /// Argument value registers (passed by value into the fragment).
        args: Vec<usize>,
    },
    /// Lazily apply a first-class function value (complete mode): builds a lazy
    /// [`Value::Callable`] (a thunk) over the callable's fragment and an env
    /// record that concatenates the callable's captured env with the argument
    /// refs. The application does NOT run — it suspends as a thunk and runs on
    /// demand.
    ValueCallLazy {
        /// Destination value register (the thunk).
        dst: usize,
        /// Value register holding the [`Value::Callable`] to apply.
        closure_slot: usize,
        /// Argument value registers (captured into the thunk's env, unforced).
        args: Vec<usize>,
    },
    /// Construct a first-class function value: allocates an eager
    /// [`Value::Callable`] (no memo cell) referencing an [`Ir::fragments`] body
    /// and the env record in `env`. Emitted when a function definition
    /// reference or lambda literal appears in value position.
    ValueMakeClosure {
        /// Destination value register.
        dst: usize,
        /// Value register holding the captured environment record ref (a
        /// dummy `Void` slot when nothing is captured).
        env: usize,
        /// Index into [`Ir::fragments`].
        fragment: usize,
    },
    /// Share a value (RC++).
    ValueCopy {
        /// Destination value register (same slot).
        dst: usize,
        /// Source value register.
        src: usize,
    },
    /// Drop a value (RC--, free at 0).
    ValueDrop {
        /// Value register to drop.
        src: usize,
    },
    /// Read a per-tick value-state slot (for `~` / `@` on values).
    ValueStateRead {
        /// Destination value register.
        dst: usize,
        /// State slot.
        slot: usize,
    },
    /// Write a per-tick value-state slot.
    ValueStateWrite {
        /// State slot.
        slot: usize,
        /// Value register.
        src: usize,
    },
    /// Dispatch on a sum constructor: yields the payload refs for the matched arm.
    ValueMatch {
        /// Destination value registers for the selected arm's payload.
        dst: Vec<usize>,
        /// Scrutinee sum value register.
        slot: usize,
        /// Constructor index to match.
        ctor: u32,
    },
    /// Boolean literal.
    ValueBool {
        /// Destination value register.
        dst: usize,
        /// The value.
        value: bool,
    },
    /// Read a named parameter slot as a Float value.
    ValueReadParam {
        /// Destination value register.
        dst: usize,
        /// Index into [`Ir::params`].
        idx: usize,
    },
    /// The unit value `()` (`Value::Void`).
    ValueConstVoid {
        /// Destination value register.
        dst: usize,
    },
    /// An opaque runtime handle constant (the stdio stream handles). Programs
    /// cannot construct opaque values themselves; only runtime-provided
    /// constants such as `stdout`/`stderr` lower to this instruction.
    ValueConstOpaque {
        /// Destination value register.
        dst: usize,
        /// The handle payload.
        handle: u32,
    },
    /// String literal.
    ValueConstString {
        /// Destination value register.
        dst: usize,
        /// The value.
        value: String,
    },
    /// List literal: alloc the container + element refs.
    ValueListLit {
        /// Destination value register.
        dst: usize,
        /// Element value registers (refs).
        elems: Vec<usize>,
    },
    /// Map literal with string keys.
    ValueMapLit {
        /// Destination value register.
        dst: usize,
        /// Key value registers (refs).
        keys: Vec<usize>,
        /// Value value registers (refs).
        vals: Vec<usize>,
    },
    /// Value-track comparison.
    ValueCompare {
        /// Destination value register.
        dst: usize,
        /// Comparison operator.
        op: CmpOp,
        /// Left operand value register.
        a: usize,
        /// Right operand value register.
        b: usize,
    },
    /// Value-track boolean logic.
    ValueLogic {
        /// Destination value register.
        dst: usize,
        /// Boolean logic operator.
        op: LogicOp,
        /// Left operand value register.
        a: usize,
        /// Right operand value register.
        b: usize,
    },
    /// Value-track boolean negation: `not b`.
    ValueNot {
        /// Destination value register.
        dst: usize,
        /// Boolean operand value register.
        src: usize,
    },
    /// Latch a `ProcessError::Processing` for this tick (runtime match
    /// non-exhaustive). The tick fails at the end of `run_value_track`.
    ValueSetError,
    /// Move an arena ref between registers (`dst = src; src = None`) — an
    /// ownership transfer used at control-flow join points.
    ValueMove {
        /// Destination value register.
        dst: usize,
        /// Source value register (cleared by the move).
        src: usize,
    },
    /// Dispatch a collection operation.
    ValueCallBuiltin {
        /// Destination value register.
        dst: usize,
        /// Collection operation.
        op: ValueBuiltinOp,
        /// Argument value registers.
        args: Vec<usize>,
    },
    /// Dispatch an effect operation.
    ///
    /// For [`ValueEffectOp::RunST`] / [`ValueEffectOp::RunSTArray`] the single
    /// argument register holds the `ST` value; the interpreter runs the whole
    /// region and writes the region's result (`runST`) or its frozen
    /// `Array` (`runSTArray`) to `dst`. For the STRef/STArray operations the
    /// arguments are the operation's operands; the interpreter builds a
    /// [`Value::BuiltinCallable`](crate::arena::Value::BuiltinCallable) capturing
    /// them and writes that to `dst` (the `unST` field of the `ST` record the
    /// lowering wraps around it).
    ValueEffectOp {
        /// Destination value register.
        dst: usize,
        /// Effect operation.
        op: ValueEffectOp,
        /// Operand value registers.
        args: Vec<usize>,
    },
    /// Create a lazy binding: allocate a lazy [`Value::Callable`] (a thunk with
    /// a memo cell) + `Value::ThunkCell(Unforced)`.
    ValueLazy {
        /// Destination value register.
        dst: usize,
        /// Captured environment record register (arena ref).
        env: usize,
        /// Index into `Ir::fragments` (the suspended body).
        fragment: usize,
    },
    /// Force a value to WHNF, result into `dst`.
    ValueForce {
        /// Destination value register.
        dst: usize,
        /// Source value register.
        src: usize,
    },
    // ---- Whole-buffer value ops (B3-2: signal programs on the value track) ----
    // Each op operates on a `Value::Buffer(usize)` value register — the buffer
    // index IS the value register index (block-register 1:1 translation). The
    // physical memory is the program's `Vec<RegionHandle<T>>` table (SP-4
    // big-object memory); mutation is in-place via `with_data_mut`.
    /// Fill the destination buffer with a constant.
    ValueBufferConst {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// Constant value to fill the whole buffer with.
        value: f64,
    },
    /// Materialize the k-th program input into the destination buffer. The
    /// input slice is copied into the buffer by `process()` before the value
    /// track runs; this instruction allocates the `Value::Buffer` marker.
    ValueBufferLoadInput {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// Program input index.
        index: usize,
    },
    /// Read the previous tick's block-state slot into the destination buffer.
    ValueBufferStateRead {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// State slot to read.
        slot: usize,
    },
    /// Read from a delay line into the destination buffer (delay 0).
    ValueBufferDelayRead {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// Delay line index.
        line: usize,
    },
    /// Unary op in place on the destination buffer.
    ValueBufferUn {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// Unary operation.
        op: UnOp,
        /// Source value register (holds `Value::Buffer(src)`).
        src: usize,
    },
    /// Binary op combining two source buffers into the destination buffer.
    ValueBufferBin {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// Binary operation.
        op: BinArith,
        /// First operand value register (holds `Value::Buffer(a)`).
        a: usize,
        /// Second operand value register (holds `Value::Buffer(b)`).
        b: usize,
    },
    /// Copy one buffer to another (wire).
    ValueBufferMove {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// Source value register (holds `Value::Buffer(src)`).
        src: usize,
    },
    /// Fill a buffer with a scalar value register's Float payload.
    ValueBufferFill {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// Source scalar value register (Float).
        src: usize,
    },
    /// Schedule a write of `src` buffer into a block-state slot (applied at
    /// tick end via the double-buffer swap).
    ValueBufferStateWrite {
        /// State slot to write.
        slot: usize,
        /// Source value register (holds `Value::Buffer(src)`).
        src: usize,
    },
    /// Push `src` buffer into a delay line.
    ValueBufferDelayWrite {
        /// Delay line index.
        line: usize,
        /// Source value register (holds `Value::Buffer(src)`).
        src: usize,
    },
    /// Call a whole-buffer built-in: `srcs` input buffers → `dst` output
    /// buffer(s) (consecutive registers for multi-output). Dispatches
    /// `Algorithm::process`/`MultichannelAlgorithm::process` over the buffers'
    /// `RegionHandle` slices (no copy at the FFI boundary).
    ///
    /// Scalar params are NOT value registers: constants are folded into
    /// `BuiltinInstance.params` at build (currying — the arrow's scalar args
    /// are applied at construction), live `?name`/`read_param` params flow via
    /// `param_bindings` → `set_param` before `process`. The `Algorithm` stays a
    /// pure function over its baked params.
    ValueCallForeign {
        /// Destination value register (first output; holds `Value::Buffer(dst)`).
        dst: usize,
        /// Source Buffer value registers (the arrow's explicit Buffer args).
        srcs: Vec<usize>,
        /// Index into [`Ir::builtins`].
        instance: usize,
    },
    /// Fill the destination buffer with a named parameter value.
    ValueBufferParamRead {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// Index into [`Ir::params`].
        idx: usize,
    },
    /// Fill the destination buffer with a main λ-parameter cell's value.
    ValueBufferMainCellRead {
        /// Destination value register (holds `Value::Buffer(dst)`).
        dst: usize,
        /// Index into the persistent main-cell store.
        cell: usize,
    },
}

/// Layout for value-track persistent storage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValueLayout {
    /// Number of arena slots pre-allocated for the whole program.
    pub capacity: usize,
    /// Ref-slot budget of the pre-allocated payload buffer pool (collection and
    /// record element buffers served by `Arena::take_buf`).
    pub buffer_budget: usize,
    /// Number of per-tick value-state slots (feedback/delay of values).
    pub value_state_slots: usize,
}

/// Value/signal arity of a function fragment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FuncSig {
    /// Number of value arguments.
    pub value_ins: usize,
    /// Number of value results.
    pub value_outs: usize,
    /// Number of signal-wire arguments.
    pub signal_ins: usize,
}

/// A compiled function body: a fragment of the value/block track.
///
/// The fragment's value instructions reference fragment-local registers
/// `0..num_value_regs`; the interpreter executes them against a temporary
/// register slice appended to the program's value register store, offsetting
/// each register field by a per-call base (the instructions are reused across
/// calls, so they are never rewritten).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FragmentIr {
    /// Value-track blocks for the body.
    pub value_blocks: Vec<ValueBlock>,
    /// Entry block of the fragment's value track.
    pub entry: usize,
    /// Number of value registers (args + temps).
    pub num_value_regs: usize,
    /// Value register(s) holding the result.
    pub output_value_regs: Vec<usize>,
    /// Number of env capture cells the call's temporary frame must hold
    /// (the lambda's free variables, one per env Record field). Drives the
    /// build-time arena capacity bound.
    pub num_capture_cells: usize,
    /// Arity.
    pub sig: FuncSig,
}

/// A named function value: reference to a lowering-time definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueFunc {
    /// Definition name.
    pub name: String,
    /// Number of value arguments.
    pub arity: usize,
}

/// Layout describing pre-allocated persistent storage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateLayout {
    /// Number of block-level feedback state slots.
    pub block_state_slots: usize,
    /// Length (in samples) of each delay line.
    pub delay_lens: Vec<usize>,
    /// Number of program outputs.
    pub num_outputs: usize,
}

/// A resolved built-in call site: its name, folded constant params, and kind.
/// Runtime instances are built from these by `RillProgram::new_with`.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinInstance {
    /// Registered built-in name.
    pub name: String,
    /// Folded constant params.
    pub params: Vec<f64>,
    /// Optional named resource (e.g. a tape loop) this built-in binds to.
    pub resource: Option<String>,
    /// Index into [`Ir::tapes`] of the shared tape cell this built-in binds to.
    /// The new `tape_loop` path resolves heads by index; the legacy named
    /// resource path uses [`Self::resource`] against an external registry.
    pub tape_index: Option<usize>,
    /// Sample vs block.
    pub kind: BuiltinKind,
    /// Number of signal input channels.
    pub signal_ins: usize,
    /// Number of signal output channels.
    pub signal_outs: usize,
    /// (arg_position, param_idx) dynamic param drivers.
    pub param_bindings: Vec<(usize, usize)>,
}

/// A named runtime parameter definition (a mutable control slot).
#[derive(Debug, Clone, PartialEq)]
pub struct ParamDef {
    /// Parameter name.
    pub name: String,
    /// Initial/default value.
    pub default: f64,
    /// Minimum (clamp lower bound).
    pub min: f64,
    /// Maximum (clamp upper bound).
    pub max: f64,
}

/// A complete lowered program.
#[derive(Debug, Clone, PartialEq)]
pub struct Ir {
    /// Whether `main`'s statically inferred result type is `IO ()` (the
    /// [`ProgramRunner`](crate::program_runner::ProgramRunner) contract).
    ///
    /// The value track's closure-cell encoding represents `()` as
    /// `Float(0.0)` (a cell's un-set state is `Value::Void`, so a stored unit
    /// reads back as `0.0`). That makes the runner's runtime payload check
    /// ambiguous: `return ()` and `return 0.0` both arrive as `Float(0.0)`.
    /// This static flag resolves the ambiguity — the erased unit encoding is
    /// accepted only when `main` is typed `IO ()`.
    pub main_io_unit: bool,
    /// Number of program inputs (0 or 1 for MVP).
    pub num_inputs: usize,
    /// Number of program outputs.
    pub num_outputs: usize,
    /// Persistent state layout.
    pub state: StateLayout,
    /// Built-in call-site descriptors, indexed by the `instance` field of
    /// [`Instr::CallSample`]/[`Instr::CallBlock`].
    pub builtins: Vec<BuiltinInstance>,
    /// Named parameter definitions, indexed by [`Instr::ReadParam::idx`].
    pub params: Vec<ParamDef>,
    /// Number of persistent main λ-parameter cells (see
    /// [`RillProgram::main_cells`](crate::program::RillProgram)). Main λ-params
    /// are the first `num_main_cells` entries of [`Ir::params`] — `set_param`
    /// on the program writes into the cell for exactly these indices.
    pub num_main_cells: usize,
    /// Value-track blocks (per-tick), see `run_value_track`.
    pub value_blocks: Vec<ValueBlock>,
    /// Entry block of the value track.
    pub value_entry: usize,
    /// Number of value registers required.
    pub num_value_regs: usize,
    /// Value registers holding program outputs.
    pub value_output_regs: Vec<usize>,
    /// Named function values referenced by [`ValueInstr::ValueCallFunc`].
    pub value_funcs: Vec<ValueFunc>,
    /// Compiled function bodies, indexed by [`ValueInstr::ValueMakeClosure`]'s
    /// `fragment` field and dispatched by [`ValueInstr::ValueCallFunc`].
    /// Function bodies: fragments of the value/block track, dispatched by
    /// [`ValueInstr::ValueCallFunc`]. Shared via `Arc` so a dispatch shares the
    /// fragment without cloning it (no heap allocation on the RT path).
    pub fragments: Vec<std::sync::Arc<FragmentIr>>,
    /// Number of value-register slots pre-allocated for the runtime function
    /// call stack: `max_call_depth × max_fragment_regs`, where
    /// `max_call_depth` is the total fragment count (a strict upper bound on
    /// the number of concurrently-active fragment frames — the acyclic
    /// contract forbids any fragment from recursing) and `max_fragment_regs`
    /// is the largest `FragmentIr::num_value_regs`. [`RillProgram`](crate::program::RillProgram)
    /// sizes its per-tick value-register store to `num_value_regs + max_call_regs`
    /// so [`run_call`](crate::backend::interp::run_call) never grows it
    /// on the RT path.
    pub max_call_regs: usize,
    /// Value-track persistent layout.
    pub value_state: ValueLayout,
    /// Number of program-level signal buffers (see
    /// [`RillProgram::buffers`](crate::program::RillProgram)). Each
    /// `Value::Buffer` index points into this table; the memory is carved from
    /// a [`BufferAllocator`](rill_core::memory::BufferAllocator) at build.
    pub num_buffers: usize,
    /// Program input → buffer-table index: input `k` is materialized by
    /// `process()` into `buffers[input_buffers[k]]` before the value track runs
    /// (parallel to [`Ir::num_inputs`]).
    pub input_buffers: Vec<usize>,
    /// Shared tape cells: one capacity per tape, indexed by
    /// [`BuiltinInstance::tape_index`]. Populated by lowering as it resolves
    /// inline `tape_loop <capacity>` constructor calls (a FRESH cell per call)
    /// and named `name = tape_loop <capacity>` declarations (one cell per
    /// name); the build allocates a `rill_core::buffer::SharedCell` per entry.
    pub tapes: Vec<usize>,
}

#[cfg(test)]
mod value_ir_tests {
    use super::*;

    #[test]
    fn value_layout_is_defaultable() {
        let l = ValueLayout::default();
        assert_eq!(l.capacity, 0);
        assert_eq!(l.value_state_slots, 0);
    }

    #[test]
    fn lazy_and_force_instructions_exist() {
        let _ = ValueInstr::ValueLazy {
            dst: 0,
            env: 1,
            fragment: 2,
        };
        let _ = ValueInstr::ValueForce { dst: 0, src: 1 };
    }
}

#[cfg(test)]
mod value_builtin_tests {
    use super::*;

    #[test]
    fn value_builtin_ops_exist() {
        let ops = [
            ValueBuiltinOp::Cons,
            ValueBuiltinOp::LazyCons,
            ValueBuiltinOp::Nil,
            ValueBuiltinOp::Head,
            ValueBuiltinOp::Tail,
            ValueBuiltinOp::Length,
            ValueBuiltinOp::Map,
            ValueBuiltinOp::Fold,
            ValueBuiltinOp::Filter,
            ValueBuiltinOp::ListEmpty,
            ValueBuiltinOp::InsertMap,
            ValueBuiltinOp::Lookup,
            ValueBuiltinOp::Member,
            ValueBuiltinOp::InsertSet,
            ValueBuiltinOp::MapEmpty,
            ValueBuiltinOp::SetEmpty,
            ValueBuiltinOp::ConcatMap,
            ValueBuiltinOp::AppendList,
            ValueBuiltinOp::ConcatString,
            ValueBuiltinOp::ShowFloat,
            ValueBuiltinOp::ShowInt,
            ValueBuiltinOp::ShowBool,
            ValueBuiltinOp::ArrayFromList,
            ValueBuiltinOp::ArrayBounds,
            ValueBuiltinOp::ArrayElems,
            ValueBuiltinOp::ArrayIndexFlat,
            ValueBuiltinOp::ArrayUpdate,
            ValueBuiltinOp::ArrayAccum,
            ValueBuiltinOp::ArrayFromBuffer,
            ValueBuiltinOp::BufferFromArray,
        ];
        assert_eq!(ops.len(), 30);
    }
}
