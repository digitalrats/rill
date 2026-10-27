//! Convenience re-exports.
//!
//! ```
//! use rill_lang::prelude::*;
//! ```

pub use crate::builtin::BuiltinKind;
pub use crate::compile;
pub use crate::compile_eval;
pub use crate::error::{CompileError, Span};
pub use crate::program::{WorldConfig, WorldState};
pub use crate::program_runner::{run_on_stack, ProgramRunner};
pub use crate::test_eval;

// The runtime dictionary value's slot and kernel vocabulary (see
// `Value::Dictionary`).
pub use crate::arena::{DictSlot, KernelId};
// The dictionary table's compile-time description and the one kernel table.
pub use crate::ir::DictDesc;
pub use crate::kernels::{has_instruction, kernel_for_method, missing_kernels, MissingKernel};
