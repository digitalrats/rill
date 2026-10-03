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
}

/// One IR instruction. `dst` is the scratch register it writes.
#[derive(Debug, Clone, PartialEq)]
pub enum Instr {
    /// Load a constant.
    Const {
        /// Destination register.
        dst: Reg,
        /// Constant value to load.
        value: f64,
    },
    /// Load the k-th program input for the current sample.
    LoadInput {
        /// Destination register.
        dst: Reg,
        /// Program input index.
        index: usize,
    },
    /// Read a persistent block-state slot (its whole block from the previous tick).
    ReadBlockState {
        /// Destination register.
        dst: Reg,
        /// State slot to read.
        slot: StateSlot,
    },
    /// Read from a delay line: value `len` samples ago.
    ReadDelay {
        /// Destination register.
        dst: Reg,
        /// Delay line index.
        line: usize,
    },
    /// Unary op.
    Un {
        /// Destination register.
        dst: Reg,
        /// Unary operation.
        op: UnOp,
        /// Source register.
        src: Reg,
    },
    /// Binary op.
    Bin {
        /// Destination register.
        dst: Reg,
        /// Binary operation.
        op: BinArith,
        /// First operand register.
        a: Reg,
        /// Second operand register.
        b: Reg,
    },
    /// Copy one register to another (wire).
    Move {
        /// Destination register.
        dst: Reg,
        /// Source register.
        src: Reg,
    },
    /// Schedule a write of `src` into a block-state slot (applied at tick end).
    WriteBlockState {
        /// State slot to write.
        slot: StateSlot,
        /// Source register.
        src: Reg,
    },
    /// Schedule a push of `src` into a delay line (whole block).
    WriteDelay {
        /// Delay line index.
        line: usize,
        /// Source register.
        src: Reg,
    },
    /// Call a whole-buffer built-in: `srcs` inputs → `dst`, instance index.
    CallBlock {
        /// Destination register (first output).
        dst: Reg,
        /// Source registers.
        srcs: Vec<Reg>,
        /// Index into [`Ir::builtins`].
        instance: usize,
    },
    /// Read a named parameter slot. Value is constant within a block.
    ReadParam {
        /// Destination register.
        dst: Reg,
        /// Index into [`Ir::params`].
        idx: usize,
    },
    /// Read an actor parameter slot (?name syntax). Semantically same as ReadParam
    /// but carries distinct semantics for higher layers (actor param naming).
    ReadActorParam {
        /// Destination register.
        dst: Reg,
        /// Index into [`Ir::params`].
        param_idx: usize,
    },
    /// Read a main λ-parameter cell, materialising its float value into a
    /// block register. The cell is persistent (allocated once at program
    /// construction), so a `SetParameter` write survives across ticks and the
    /// block track reads the current value directly each block.
    ReadMainCell {
        /// Destination register.
        dst: Reg,
        /// Index into the persistent main-cell store ([`Ir::num_main_cells`]).
        cell: usize,
    },
    /// A debug probe point that passes a signal through unchanged.
    /// The runtime debug engine can latch this value for inspection.
    #[cfg(feature = "debug")]
    ProbePoint {
        /// Unique probe identifier.
        id: ProbeId,
        /// Source register to copy from.
        src: Reg,
        /// Destination register to write to.
        dst: Reg,
    },
}

/// A value-track collection operation dispatched by the interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueBuiltinOp {
    /// Prepend an element to a list.
    Cons,
    /// Read the first element of a list (a `Maybe`).
    Head,
    /// Drop the first element of a list.
    Tail,
    /// Count the elements of a list/set/map.
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
    /// [`run_fragment`](crate::backend::interp::run_fragment)) when
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
    /// [`FragmentIr`] referenced by the closure's `fragment` id, binding the
    /// caller's argument registers and copying the fragment's result into `dst`.
    ValueCallFunc {
        /// Destination value register.
        dst: usize,
        /// Value register holding the [`Value::Closure`] to call.
        closure_slot: usize,
        /// Argument value registers (passed by value into the fragment).
        args: Vec<usize>,
    },
    /// Construct a first-class function value: allocates a [`Value::Closure`]
    /// referencing an [`Ir::fragments`] body and the env record in `env`.
    /// Emitted when a function definition reference or lambda literal appears
    /// in value position.
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
    /// Block-track steps (for signal-wire args); empty for pure-value bodies.
    pub steps: Vec<crate::schedule::Step>,
    /// Number of value registers (args + temps).
    pub num_value_regs: usize,
    /// Number of block registers (signal args + temps).
    pub num_block_regs: usize,
    /// Value register(s) holding the result.
    pub output_value_regs: Vec<usize>,
    /// Block register(s) holding signal results.
    pub output_block_regs: Vec<usize>,
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
    /// Instructions in evaluation order.
    pub instrs: Vec<Instr>,
    /// Number of scratch registers required.
    pub num_regs: usize,
    /// The registers holding the program outputs at sample end, in output order.
    pub output_regs: Vec<Reg>,
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
    /// so [`run_fragment`](crate::backend::interp::run_fragment) never grows it
    /// on the RT path.
    pub max_call_regs: usize,
    /// Value-track persistent layout.
    pub value_state: ValueLayout,
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
}

#[cfg(test)]
mod value_builtin_tests {
    use super::*;

    #[test]
    fn value_builtin_ops_exist() {
        let ops = [
            ValueBuiltinOp::Cons,
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
        ];
        assert_eq!(ops.len(), 17);
    }
}
