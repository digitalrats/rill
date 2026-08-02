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
pub mod render;
pub mod runtime;
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

/// Compile an already-parsed AST `Program` into a graph engine that supports SetParameter.
pub fn compile_program<T: Transcendental, const BUF_SIZE: usize>(
    program: &crate::ast::Program,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<graph_engine::CompiledGraphEngine<T, BUF_SIZE>, CompileError> {
    let mut typed = types::infer::infer_program_with(program, registry)?;
    typed.program = reduce::reduce(&typed.program);
    let ir = lower::lower_with(&typed, registry, sample_rate)?;
    validate_block_builtins(&ir)?;

    use crate::graph_ir::{GraphIr, GraphNode};
    let mut nodes: indexmap::IndexMap<String, GraphNode> = indexmap::IndexMap::new();
    let params = ir.params.clone();
    nodes.insert(
        "main".to_string(),
        GraphNode {
            arity: (ir.num_inputs, ir.num_outputs),
            ir,
            params,
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

    let compiled = graph_compiler::compile::<T, BUF_SIZE>(&graph_ir, registry, sample_rate)
        .map_err(CompileError::Unsupported)?;

    let mailbox = Arc::new(Mailbox::new(64));
    Ok(graph_engine::CompiledGraphEngine::new(compiled, mailbox))
}

/// Compile rill-lang source into a graph engine that supports SetParameter.
pub fn compile_graph<T: Transcendental, const BUF_SIZE: usize>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<graph_engine::CompiledGraphEngine<T, BUF_SIZE>, CompileError> {
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

    let compiled = graph_compiler::compile::<T, BUF_SIZE>(&graph_ir, registry, sample_rate)
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

#[cfg(test)]
mod ir_tests {
    use super::*;

    #[test]
    fn lang_chiptune_ir_structure() {
        use crate::builtin::{BuiltinKind, BuiltinSig, Registry};

        let mut registry = Registry::<f32>::new();
        registry.register_block(
            BuiltinSig::simple("ay38910", 0, 1, 2, BuiltinKind::Block),
            |_, _| panic!("not instantiated"),
        );
        // lofi: 1 signal in (from pipeline :), 1 out, 7 params
        registry.register_block(
            BuiltinSig::simple("lofi", 1, 1, 7, BuiltinKind::Block),
            |_, _| panic!("not instantiated"),
        );

        let src = r"main regs = ay38910 1750000.0 regs : lofi 8 44100 0.75 1.0 1 0 1";
        let tokens = lexer::tokenize(src).unwrap();
        let program = parser::parse(&tokens, src.as_bytes()).unwrap();
        let mut typed = types::infer::infer_program_with(&program, &registry).unwrap();
        typed.program = reduce::reduce(&typed.program);
        let ir = lower::lower_with(&typed, &registry, 44100.0).unwrap();

        eprintln!("=== DSL Ir for lang_chiptune ===");
        eprintln!("num_inputs: {}", ir.num_inputs);
        eprintln!("num_outputs: {}", ir.num_outputs);
        eprintln!("num_regs: {}", ir.num_regs);
        eprintln!("output_reg: {:?}", ir.output_reg);
        for (i, bi) in ir.builtins.iter().enumerate() {
            eprintln!(
                "builtin[{i}]: name={}, kind={:?}, si={}, so={}, params={:?}, bindings={:?}",
                bi.name, bi.kind, bi.signal_ins, bi.signal_outs, bi.params, bi.param_bindings
            );
        }
        eprintln!(
            "params: {:?}",
            ir.params.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
        for (i, instr) in ir.instrs.iter().enumerate() {
            eprintln!("instr[{i}]: {:?}", instr);
        }
    }
}
