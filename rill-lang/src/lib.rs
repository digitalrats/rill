//! # rill-lang
//!
//! A Faust-style functional streaming DSL that compiles to a
//! [`rill_core::Algorithm`]. See the crate guide for language details.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod arena;
/// The signal-arrow core: combinators as arrow laws over [`arrow::ArrowTy`].
pub mod arrow;
pub mod ast;
pub mod backend;
pub mod builtin;
/// Built-in multi-IO signal processors (mixer, EQ, dry/wet).
pub mod builtins;
/// Faust-combinator sugar: foreign builtins keep their legacy arrow call style.
mod desugar;
pub mod error;
/// FFI factory registry: foreign-declared builtins' Rust implementations.
pub mod ffi;
/// Graph IR formation: [`GraphSpec`](graph::GraphSpec) → [`CompiledStream`](graph::CompiledStream).
pub mod graph;
pub mod ir;
pub mod lexer;
pub mod lower;
pub mod parser;
pub mod prelude;
pub mod program;
pub mod program_engine;
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

pub use arena::{Arena, ArenaError, ArenaRef, Value, ValueKind};
pub use error::{CompileError, Span};
pub use ir::{Instr, Ir, ValueFunc, ValueInstr, ValueLayout};
pub use program::RillProgram;
pub use serde_def::{compile_def, RillLangDef};

pub use builtin::{BuiltinKind, Registry};

use rill_core::math::Transcendental;
use rill_core_actor::Mailbox;
use std::sync::Arc;

/// Compile rill-lang source into a runnable [`RillProgram`] for scalar type `T`.
///
/// Uses the runtime-safe default block size (`BUF = 256`); each block passed to
/// `process` must be no longer than that.
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
pub fn compile<T: Transcendental>(src: &str) -> Result<RillProgram<T, 256>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let (program, tape_decls) = extract_resources(&program)?;
    let mut typed = types::infer::infer_program(&program)?;
    typed.tape_decls = tape_decls
        .iter()
        .map(|d| (d.name.clone(), d.capacity))
        .collect();
    typed.program = reduce::reduce_with_cafs(&typed.program, &typed.cafs);
    let ir = lower::lower_with_cafs(&typed, 44_100.0, &typed.cafs)?;
    // regalloc::allocate(&mut ir);
    Ok(RillProgram::<T, 256>::new(ir))
}

/// Compile with a built-in registry and a sample rate. Uses the default block
/// size (`BUF = 256`).
pub fn compile_with<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<RillProgram<T, 256>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let (program, tape_decls) = extract_resources(&program)?;
    let mut typed = types::infer::infer_program_with(&program)?;
    typed.tape_decls = tape_decls
        .iter()
        .map(|d| (d.name.clone(), d.capacity))
        .collect();
    typed.program = reduce::reduce_with_cafs(&typed.program, &typed.cafs);
    let ir = lower::lower_with_cafs(&typed, sample_rate, &typed.cafs)?;
    // regalloc::allocate(&mut ir);
    RillProgram::<T, 256>::new_with(ir, registry, sample_rate, None)
}

/// Compile source against a foreign registry: `foreign fn` builtins resolve
/// their Rust implementations from `ffi` (a fresh, empty legacy built-in
/// registry is used for the built-in path). Uses the default block size
/// (`BUF = 256`).
pub fn compile_with_ffi<T: Transcendental>(
    src: &str,
    ffi: &crate::ffi::ForeignRegistry<T>,
    sample_rate: f32,
) -> Result<RillProgram<T, 256>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let (program, tape_decls) = extract_resources(&program)?;
    let mut typed = types::infer::infer_program(&program)?;
    typed.tape_decls = tape_decls
        .iter()
        .map(|d| (d.name.clone(), d.capacity))
        .collect();
    typed.program = reduce::reduce_with_cafs(&typed.program, &typed.cafs);
    let ir = lower::lower_with_cafs(&typed, sample_rate, &typed.cafs)?;
    let registry = Registry::<T>::new();
    RillProgram::<T, 256>::new_with(ir, &registry, sample_rate, Some(ffi))
}

/// Compile an already-parsed AST `Program` into a graph engine that supports SetParameter.
pub fn compile_program<T: Transcendental, const BUF: usize>(
    program: &crate::ast::Program,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<program_engine::ProgramEngine<T, BUF>, CompileError> {
    compile_program_inner(program, registry, sample_rate, None)
}

/// Compile an AST program against a pre-built resource registry.
///
/// The registry is shared (e.g. one tape across recording + playback engines);
/// the caller owns it and must keep it alive while both engines run. The DSL's
/// `TapeLoop <capacity>` declaration is only a declaration: when an external
/// registry is supplied, its tape capacity is used. Every resource referenced
/// by the program must exist in `resources`, otherwise the program fails with
/// `Unsupported` rather than compiling to a silently dead engine (a write head
/// without a writer, a read head without a reader).
pub fn compile_program_with_resources<T: Transcendental, const BUF: usize>(
    program: &crate::ast::Program,
    registry: &Registry<T>,
    sample_rate: f32,
    resources: &mut rill_core::buffer::ResourceRegistry<T>,
) -> Result<program_engine::ProgramEngine<T, BUF>, CompileError> {
    compile_program_inner(program, registry, sample_rate, Some(resources))
}

fn compile_program_inner<T: Transcendental, const BUF: usize>(
    program: &crate::ast::Program,
    registry: &Registry<T>,
    sample_rate: f32,
    mut resources: Option<&mut rill_core::buffer::ResourceRegistry<T>>,
) -> Result<program_engine::ProgramEngine<T, BUF>, CompileError> {
    let (program, resource_decls) = extract_resources(program)?;

    let mut typed = types::infer::infer_program_with(&program)?;
    typed.program = reduce::reduce_with_cafs(&typed.program, &typed.cafs);

    // The DSL `tape_loop` path (no external registry) resolves a resource
    // param's `Ref(name)` to a tape INDEX via the extracted declarations. The
    // caller-supplied-registry path (graph duplex) keeps name-based bindings —
    // its `Ref`s resolve against the shared external registry at build.
    if resources.is_none() {
        typed.tape_decls = resource_decls
            .iter()
            .map(|d| (d.name.clone(), d.capacity))
            .collect();
    }
    let ir = lower::lower_with_cafs(&typed, sample_rate, &typed.cafs)?;

    if let Some(res) = &mut resources {
        for bi in &ir.builtins {
            if let Some(name) = &bi.resource {
                if res.reader(name).is_none() {
                    return Err(CompileError::Unsupported(format!(
                        "resource '{}' not found in the provided registry",
                        name
                    )));
                }
            }
        }
        let rp = RillProgram::<T, BUF>::new_with_resources(ir, registry, sample_rate, res, None)?;
        let mailbox = Arc::new(Mailbox::new(64));
        return Ok(program_engine::ProgramEngine::<T, BUF>::new(rp, mailbox));
    }

    // DSL path: every tape binding must resolve to a declared `tape_loop` cell.
    for bi in &ir.builtins {
        if let Some(res) = &bi.resource {
            return Err(CompileError::Unsupported(format!(
                "built-in '{}' references undeclared resource '{}' (declare it with `tape = tape_loop <capacity>` or pass a registry)",
                bi.name, res
            )));
        }
    }
    let rp = RillProgram::<T, BUF>::new_with(ir, registry, sample_rate, None)?;
    let mailbox = Arc::new(Mailbox::new(64));
    Ok(program_engine::ProgramEngine::<T, BUF>::new(rp, mailbox))
}

/// Compile rill-lang source into a graph engine that supports SetParameter.
///
/// `BUF` is the block size the caller will feed the engine each tick; all
/// internal buffers are pre-allocated to this size at construction.
pub fn compile_graph<T: Transcendental, const BUF: usize>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<program_engine::ProgramEngine<T, BUF>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    compile_program::<T, BUF>(&program, registry, sample_rate)
}

/// A named resource declaration (e.g. a tape loop) from the DSL.
pub struct ResourceDecl {
    /// Resource name.
    pub name: String,
    /// Capacity in samples (for tape loops).
    pub capacity: usize,
}

/// Extract top-level `name = tape_loop <capacity>` (and the legacy
/// `name = TapeLoop <capacity>`) resource declarations, returning the remaining
/// signal program plus the declarations.
///
/// The `TapeLoop` spelling is kept for the graph-duplex path
/// (`graph/compile.rs::render_recording` still emits it against the
/// externally-shared `ResourceRegistry`); the DSL path uses `tape_loop`.
fn extract_resources(
    program: &crate::ast::Program,
) -> Result<(crate::ast::Program, Vec<ResourceDecl>), CompileError> {
    use crate::ast::{Def, Expr};
    let mut decls = Vec::new();
    let mut defs = Vec::with_capacity(program.defs.len());
    for def in &program.defs {
        let mut is_resource = false;
        // Never treat `main` as a resource declaration — `main = tape_loop
        // <capacity>` is a plausible typo that must surface as a type error on
        // `main`, not as a missing-`main` error.
        if def.name() == "main" {
            defs.push(def.clone());
            continue;
        }
        if let Def::Local {
            name,
            body: Expr::Apply {
                name: ctor, args, ..
            },
            ..
        } = def
        {
            if ctor == "tape_loop" || ctor == "TapeLoop" {
                match args.as_slice() {
                    [Expr::Int(v, _)] => {
                        let cap = *v as usize;
                        if cap == 0 {
                            return Err(CompileError::Unsupported(format!(
                                "tape '{}' has zero capacity",
                                name
                            )));
                        }
                        decls.push(ResourceDecl {
                            name: name.clone(),
                            capacity: cap,
                        });
                        is_resource = true;
                    }
                    _ => {
                        return Err(CompileError::Unsupported(format!(
                            "tape '{}' declaration needs a single positive integer capacity",
                            name
                        )));
                    }
                }
            }
        }
        if !is_resource {
            defs.push(def.clone());
        }
    }
    Ok((crate::ast::Program { defs }, decls))
}

#[cfg(test)]
mod ir_tests {
    use super::*;
    use crate::builtin::Registry;

    struct TestOsc;
    impl rill_core::traits::Algorithm<f32> for TestOsc {
        fn process(
            &mut self,
            _input: Option<&[f32]>,
            output: &mut [f32],
        ) -> rill_core::traits::ProcessResult<()> {
            output.fill(0.0);
            Ok(())
        }
        fn reset(&mut self) {}
    }
    impl crate::builtin::BlockBuiltin<f32> for TestOsc {}

    fn sine_registry() -> Registry<f32> {
        let mut registry = Registry::<f32>::new();
        registry.register_block("sine", |_, _| Box::new(TestOsc));
        registry
    }

    #[test]
    fn public_compile_rejects_recursive_caf() {
        // a = a -> graceful CompileError, not stack overflow
        let res = compile::<f32>("a = a; main = a");
        assert!(res.is_err());
    }

    #[test]
    fn public_compile_with_shares_closed_caf() {
        // osc = sine 440.0 1.0 0.0; main = osc , osc -> ONE sine instance
        let registry = sine_registry();
        let prog = compile_with::<f32>(
            "osc = sine 440.0 1.0 0.0; main = osc , osc",
            &registry,
            44100.0,
        )
        .unwrap();
        let sines = prog.ir.builtins.iter().filter(|b| b.name == "sine").count();
        assert_eq!(sines, 1);
        assert_eq!(prog.ir.builtins.len(), 1);
    }

    #[test]
    fn lang_chiptune_ir_structure() {
        let mut registry = Registry::<f32>::new();
        registry.register_block("ay38910", |_, _| panic!("not instantiated"));
        // lofi: 1 signal in (from pipeline :), 1 out, 7 params
        registry.register_block("lofi", |_, _| panic!("not instantiated"));

        let src = r"main regs = ay38910 1750000.0 regs : lofi 8 44100 0.75 1.0 1 0 1";
        let tokens = lexer::tokenize(src).unwrap();
        let program = parser::parse(&tokens, src.as_bytes()).unwrap();
        let mut typed = types::infer::infer_program_with(&program).unwrap();
        typed.program = reduce::reduce(&typed.program);
        let ir = lower::lower_with(&typed, 44100.0).unwrap();
        assert!(ir.num_inputs > 0);
        assert!(ir.num_outputs > 0);
    }
}
