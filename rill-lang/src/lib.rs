//! # rill-lang
//!
//! A Faust-style functional streaming DSL that compiles to a
//! [`rill_core::Algorithm`]. See the crate guide for language details.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod ast;
pub mod backend;
pub mod builtin;
/// Built-in multi-IO signal processors (mixer, EQ, dry/wet).
pub mod builtins;
pub mod error;
pub mod graph_engine;
pub mod graph_ir;
pub mod graph_lower;
pub mod graph_optimize;
pub mod ir;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod prelude;
pub mod program;
pub mod program_runner;
pub mod reduce;
pub mod regalloc;
pub mod register;
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
use rill_core_actor::Mailbox;
use std::sync::Arc;

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
    use crate::graph_lower::{ScheduledGraph, Step};

    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let mut typed = types::infer::infer_program_with(&program, registry)?;

    typed.program = reduce::reduce(&typed.program);
    let ir = lower::lower_with(&typed, registry, sample_rate)?;
    // regalloc::allocate(&mut ir);
    validate_block_builtins(&ir)?;
    let prog = RillProgram::<T>::new_with(ir, registry, sample_rate)?;

    let n_params = prog.params_meta().len();

    let mut step_input = Vec::new();
    if prog.ir.num_inputs > 0 {
        step_input.push(0u32 as usize); // buffer 0 = graph input
    }

    let schedule = ScheduledGraph {
        inputs: 1,
        outputs: 1,
        steps: vec![Step::InlineProgram {
            node_idx: 0,
            input_bufs: step_input,
            output_bufs: vec![1],
            param_indices: (0..n_params).collect(),
        }],
        buffers: 2,
        output_mapping: vec![1],
        program_names: vec!["main".to_string()],
    };

    let mailbox = Arc::new(Mailbox::new(64));

    Ok(graph_engine::RillGraphEngine::new(
        schedule,
        vec![prog],
        mailbox,
        512,
    ))
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
