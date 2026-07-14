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
pub mod graph_compiler;
pub mod graph_engine;
pub mod graph_ir;
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

#[cfg(feature = "debug")]
pub mod debug;

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

/// Compile rill-lang source into a graph engine that supports SetParameter.
pub fn compile_graph<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<graph_engine::CompiledGraphEngine<T, 512>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let mut typed = types::infer::infer_program_with(&program, registry)?;
    typed.program = reduce::reduce(&typed.program);
    let ir = lower::lower_with(&typed, registry, sample_rate)?;
    validate_block_builtins(&ir)?;

    use crate::graph_ir::{GraphIr, GraphNode};
    let mut nodes: indexmap::IndexMap<String, GraphNode> = indexmap::IndexMap::new();
    nodes.insert(
        "main".to_string(),
        GraphNode {
            arity: (ir.num_inputs, ir.num_outputs),
            ir,
            params: vec![],
            keep: false,
            inline: false,
            is_bridge: false,
            feedback_read: vec![],
            feedback_write: vec![],
        },
    );
    let graph_ir = GraphIr {
        inputs: 1,
        outputs: 1,
        nodes,
        edges: vec![],
        topo_order: vec!["main".to_string()],
    };

    let compiled = graph_compiler::compile::<T, 512>(&graph_ir, registry, sample_rate)
        .map_err(CompileError::Unsupported)?;

    let mailbox = Arc::new(Mailbox::new(64));
    Ok(graph_engine::CompiledGraphEngine::new(compiled, mailbox))
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
