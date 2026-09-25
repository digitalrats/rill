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
    ValueReadCell {
        /// Destination value register.
        dst: usize,
        /// Value register holding the cell ref.
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
    /// Call a named function reference.
    ValueCallFunc {
        /// Destination value register.
        dst: usize,
        /// Index into [`Ir::value_funcs`].
        func: usize,
        /// Argument value registers.
        args: Vec<usize>,
    },
    /// Construct a first-class named-function value: allocates a [`Value::Func`]
    /// referencing the [`Ir::value_funcs`] entry. Emitted when a bare user
    /// definition reference (`f = double`, `main = f`) appears in value
    /// position. v1 calls the referenced function by β-reducing at compile
    /// time, so [`ValueCallFunc`] remains a no-op for runtime dispatch.
    ValueMakeFunc {
        /// Destination value register.
        dst: usize,
        /// Index into [`Ir::value_funcs`].
        func: usize,
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
}

/// Layout for value-track persistent storage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValueLayout {
    /// Number of arena slots pre-allocated for the whole program.
    pub capacity: usize,
    /// Number of per-tick value-state slots (feedback/delay of values).
    pub value_state_slots: usize,
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
    /// Value-track instructions (per-tick).
    pub value_instrs: Vec<ValueInstr>,
    /// Number of value registers required.
    pub num_value_regs: usize,
    /// Value registers holding program outputs.
    pub value_output_regs: Vec<usize>,
    /// Named function values referenced by [`ValueInstr::ValueCallFunc`].
    pub value_funcs: Vec<ValueFunc>,
    /// Value-track persistent layout.
    pub value_state: ValueLayout,
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
