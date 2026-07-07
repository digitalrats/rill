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
pub mod graph_build;
pub mod graph_engine;
pub mod graph_ir;
pub mod graph_lower;
pub mod graph_optimize;
pub mod graph_schedule;
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

pub use builtin::{BuiltinKind, BuiltinSig, Registry, SampleBuiltin};

use rill_core::math::Transcendental;
use rill_core_actor::ActorSystem;

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

/// Compile with a built-in registry and a sample rate.
pub fn compile_with<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<RillProgram<T>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens)?;
    let typed = types::infer::infer_program_with(&program, registry)?;
    let ir = lower::lower_with(&typed, registry, sample_rate)?;
    validate_block_builtins(&ir)?;
    RillProgram::<T>::new_with(ir, registry, sample_rate)
}

/// Compile a rill-lang source that may contain `param`/`keep param`/`inline param`
/// graph-node definitions into a `RillGraphEngine`.
///
/// If the source has no `param`-annotated definitions, falls back to single-algorithm
/// mode (wrapping `compile_with` in a graph engine with one inline step).
///
/// The `BUF` const generic must match the block size of the I/O backend.
pub fn compile_graph<T: Transcendental, const BUF: usize>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
    _system: &ActorSystem,
) -> Result<graph_engine::RillGraphEngine<T, BUF>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens)?;
    let typed = types::infer::infer_program_with(&program, registry)?;
    let mut graph_ir = graph_build::build_graph_ir(&typed)?;

    // Single-algorithm fallback: no param nodes
    if graph_ir.nodes.is_empty() {
        let ir = lower::lower_with(&typed, registry, sample_rate)?;
        validate_block_builtins(&ir)?;
        let prog = RillProgram::<T>::new_with(ir, registry, sample_rate)?;
        let param_count = prog.params_meta().len();

        let schedule = graph_schedule::ScheduledGraph {
            inputs: 1,
            outputs: 1,
            steps: vec![graph_schedule::Step::InlineProgram {
                node_idx: 0,
                input_bufs: vec![0],
                output_bufs: vec![1],
                param_indices: (0..param_count).collect(),
            }],
            buffers: 2,
            delay_slots: 0,
            output_mapping: vec![1],
        };

        return Ok(graph_engine::RillGraphEngine::new(
            schedule,
            vec![prog],
            vec!["process".to_string()],
        ));
    }

    // Graph path: optimize → lower → schedule → build engine
    graph_optimize::optimize(&mut graph_ir);
    let order = graph_lower::lower_graph(&graph_ir)?;
    let schedule = graph_schedule::build_scheduled_graph(&graph_ir, &order);

    // Compile each node into a RillProgram
    let node_names: Vec<String> = graph_ir.nodes.keys().cloned().collect();
    let mut programs = Vec::with_capacity(graph_ir.nodes.len());
    for name in &node_names {
        let node = &graph_ir.nodes[name];
        let prog = RillProgram::<T>::new_with(node.ir.clone(), registry, sample_rate)?;
        programs.push(prog);
    }

    Ok(graph_engine::RillGraphEngine::new(
        schedule, programs, node_names,
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
