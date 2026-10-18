//! Convenience re-exports.
//!
//! ```
//! use rill_lang::prelude::*;
//! ```

pub use crate::builtin::{BuiltinKind, Registry};
pub use crate::compile;
pub use crate::compile_with;
pub use crate::error::{CompileError, Span};
pub use crate::program::{RillProgram, WorldConfig, WorldState};
pub use crate::program_runner::{run_on_stack, ProgramRunner};
