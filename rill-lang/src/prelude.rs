//! Convenience re-exports.
//!
//! ```
//! use rill_lang::prelude::*;
//! ```

pub use crate::builtin::{BuiltinKind, Registry};
pub use crate::compile;
pub use crate::compile_eval;
pub use crate::error::{CompileError, Span};
pub use crate::program::{WorldConfig, WorldState};
pub use crate::program_runner::{run_on_stack, ProgramRunner};
pub use crate::test_eval;
