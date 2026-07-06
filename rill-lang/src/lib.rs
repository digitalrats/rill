//! # rill-lang
//!
//! A Faust-style functional streaming DSL that compiles to a
//! [`rill_core::Algorithm`]. See the crate guide for language details.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod ast;
pub mod backend;
pub mod builtin;
pub mod error;
pub mod ir;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod prelude;
pub mod program;
pub mod schedule;
pub mod serde_def;
pub mod types;

pub use error::{CompileError, Span};
pub use program::RillProgram;
pub use serde_def::{compile_def, RillLangDef};

use rill_core::math::Transcendental;

/// Compile rill-lang source into a runnable [`RillProgram`] for scalar type `T`.
///
/// ```
/// use rill_lang::compile;
/// use rill_core::traits::Algorithm;
///
/// let mut prog = compile::<f32>("process = _ * 0.5;").unwrap();
/// let mut out = [0.0f32; 2];
/// prog.process(Some(&[2.0, 4.0]), &mut out).unwrap();
/// assert_eq!(out, [1.0, 2.0]);
/// ```
pub fn compile<T: Transcendental>(src: &str) -> Result<RillProgram<T>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens)?;
    let typed = types::infer::infer_program(&program)?;
    let ir = lower::lower(&typed)?;
    Ok(RillProgram::<T>::new(ir))
}
