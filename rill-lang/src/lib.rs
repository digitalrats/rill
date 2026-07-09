//! # rill-lang
//!
//! A Faust-style functional streaming DSL that compiles to a
//! [`rill_core::Algorithm`]. See the crate guide for language details.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod ast;
pub mod backend;
pub mod builtin;
pub mod builtins;
pub mod error;
pub mod graph_engine;
pub mod ir;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod prelude;
pub mod program;
pub mod program_runner;
pub mod reduce;
pub mod regalloc;
pub mod schedule;
pub mod serde_def;
pub mod types;

pub use error::{CompileError, Span};
pub use program::RillProgram;
pub use serde_def::{compile_def, RillLangDef};

pub use builtin::{
    BuiltinKind, BuiltinSig, ParamType, RecordField, RecordSchema, Registry, SampleBuiltin,
};

use rill_core::math::Transcendental;
use std::collections::HashMap;

/// Compile rill-lang source into a runnable [`RillProgram`] for scalar type `T`.
///
/// ```
/// use rill_lang::compile;
/// use rill_core::traits::Algorithm;
///
/// let mut prog = compile::<f32>("main = _ * 0.5").unwrap();
/// let mut out = [0.0f32; 2];
/// prog.process(Some(&[2.0, 4.0]), &mut out).unwrap();
/// assert_eq!(out, [1.0, 2.0]);
/// ```
pub fn compile<T: Transcendental>(src: &str) -> Result<RillProgram<T>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let mut typed = types::infer::infer_program(&program)?;
    typed.program = reduce::reduce(&typed.program);
    let ir = lower::lower(&typed)?;
    // regalloc::allocate(&mut ir);
    Ok(RillProgram::<T>::new(ir))
}

/// Compile with a built-in registry and a sample rate.
pub fn compile_with<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<RillProgram<T>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let mut typed = types::infer::infer_program_with(&program, registry)?;
    typed.program = reduce::reduce(&typed.program);
    let ir = lower::lower_with(&typed, registry, sample_rate)?;
    // regalloc::allocate(&mut ir);
    validate_block_builtins(&ir)?;
    RillProgram::<T>::new_with(ir, registry, sample_rate)
}

/// Compile rill-lang source into an engine that supports [`SetParameter`]
/// commands via mailbox for runtime parameter updates.
///
/// Main parameters are addressed by name directly. Where-block anchor
/// parameters use the format `"anchor.param"` (dot-separated).
///
/// [`SetParameter`]: rill_core::queues::CommandEnum::SetParameter
pub fn compile_graph<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<graph_engine::RillGraphEngine<T>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let mut typed = types::infer::infer_program_with(&program, registry)?;

    typed.program = reduce::reduce(&typed.program);
    let ir = lower::lower_with(&typed, registry, sample_rate)?;
    // regalloc::allocate(&mut ir);
    validate_block_builtins(&ir)?;
    let prog = RillProgram::<T>::new_with(ir, registry, sample_rate)?;

    let mut p_idx: usize = 0;
    let mut param_map: HashMap<String, usize> = HashMap::new();

    let main = program.main_def().ok_or_else(|| CompileError::Parse {
        msg: "program must contain a `main` definition".into(),
        span: Span::new(0, 0),
    })?;

    for p in main.params() {
        param_map.insert(p.name.clone(), p_idx);
        p_idx += 1;
    }
    for def in main.where_defs() {
        if let crate::ast::Def::Anchor { name, params, .. } = def {
            for p in params {
                param_map.insert(format!("{}.{}", name, p.name), p_idx);
                p_idx += 1;
            }
        }
    }

    Ok(graph_engine::RillGraphEngine::new(prog, param_map))
}

fn validate_block_builtins(ir: &crate::ir::Ir) -> Result<(), CompileError> {
    use crate::ir::Instr;
    use crate::schedule::{build_schedule, Step};
    for instr in &ir.instrs {
        if let Instr::CallSample { srcs, .. } = instr {
            if srcs.len() > backend::interp::MAX_SAMPLE_BUILTIN_INS {
                return Err(CompileError::Unsupported(format!(
                    "sample built-in has {} signal inputs; the maximum is {}",
                    srcs.len(),
                    backend::interp::MAX_SAMPLE_BUILTIN_INS,
                )));
            }
        }
    }
    let sched = build_schedule(ir);
    for step in &sched.steps {
        if let Step::Sample(instrs) = step {
            for &idx in instrs {
                if matches!(ir.instrs[idx], Instr::CallBlock { .. }) {
                    return Err(CompileError::Unsupported(
                        "block built-in cannot be used inside a feedback loop (`~`)".to_string(),
                    ));
                }
            }
        }
    }
    Ok(())
}
