//! Compilation backends: turn IR into a runnable `RillProgram`.

pub mod interp;

use rill_core::math::Transcendental;

use crate::error::CompileError;
use crate::ir::Ir;
use crate::program::RillProgram;

/// A backend builds a runnable program from lowered IR.
pub trait Backend {
    /// Build a program for scalar type `T`.
    fn build<T: Transcendental>(&self, ir: Ir) -> Result<RillProgram<T>, CompileError>;
}

/// The default safe interpreter backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct InterpBackend;

impl Backend for InterpBackend {
    fn build<T: Transcendental>(&self, ir: Ir) -> Result<RillProgram<T>, CompileError> {
        Ok(RillProgram::new(ir))
    }
}
