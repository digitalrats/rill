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

/// A slot index into the persistent state vector.
pub type StateSlot = usize;

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
    /// Read a persistent state slot (its value from the *previous* sample).
    ReadState {
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
    /// Schedule a write of `src` into state slot at end of the sample.
    WriteState {
        /// State slot to write.
        slot: StateSlot,
        /// Source register.
        src: Reg,
    },
    /// Schedule a push of `src` into a delay line at end of the sample.
    WriteDelay {
        /// Delay line index.
        line: usize,
        /// Source register.
        src: Reg,
    },
    /// Call a per-sample built-in: `srcs` inputs → `dst`, instance index.
    CallSample {
        /// Destination register.
        dst: Reg,
        /// Source registers.
        srcs: Vec<Reg>,
        /// Index into [`Ir::builtins`].
        instance: usize,
    },
    /// Call a whole-buffer built-in (1→1): `src` → `dst`, instance index.
    CallBlock {
        /// Destination register.
        dst: Reg,
        /// Source register.
        src: Reg,
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
}

/// Layout describing pre-allocated persistent storage.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateLayout {
    /// Number of scalar feedback state slots.
    pub state_slots: usize,
    /// Length (in samples) of each delay line.
    pub delay_lens: Vec<usize>,
}

/// A resolved built-in call site: its name, folded constant params, and kind.
/// Runtime instances are built from these by `RillProgram::new_with`.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinInstance {
    /// Registered built-in name.
    pub name: String,
    /// Folded constant params.
    pub params: Vec<f64>,
    /// Sample vs block.
    pub kind: BuiltinKind,
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
    /// The register holding the single program output at sample end.
    pub output_reg: Reg,
    /// Number of program inputs (0 or 1 for MVP).
    pub num_inputs: usize,
    /// Persistent state layout.
    pub state: StateLayout,
    /// Built-in call-site descriptors, indexed by the `instance` field of
    /// [`Instr::CallSample`]/[`Instr::CallBlock`].
    pub builtins: Vec<BuiltinInstance>,
    /// Named parameter definitions, indexed by [`Instr::ReadParam::idx`].
    pub params: Vec<ParamDef>,
}
