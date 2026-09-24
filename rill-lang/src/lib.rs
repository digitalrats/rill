//! # rill-lang
//!
//! A Faust-style functional streaming DSL that compiles to a
//! [`rill_core::Algorithm`]. See the crate guide for language details.

#![deny(unsafe_code)]
#![warn(missing_docs)]

/// The signal-arrow core: combinators as arrow laws over [`arrow::ArrowTy`].
pub mod arrow;
pub mod ast;
pub mod backend;
pub mod builtin;
/// Built-in multi-IO signal processors (mixer, EQ, dry/wet).
pub mod builtins;
pub mod error;
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

pub use error::{CompileError, Span};
pub use program::RillProgram;
pub use serde_def::{compile_def, RillLangDef};

pub use builtin::{BuiltinKind, BuiltinSig, ParamType, RecordField, RecordSchema, Registry};

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
    let mut typed = types::infer::infer_program(&program)?;
    typed.program = reduce::reduce_with_cafs(&typed.program, &typed.cafs);
    let ir = lower::lower_with_cafs(&typed, &crate::builtin::NoSigs, 44_100.0, &typed.cafs)?;
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
    let mut typed = types::infer::infer_program_with(&program, registry)?;
    typed.program = reduce::reduce_with_cafs(&typed.program, &typed.cafs);
    let ir = lower::lower_with_cafs(&typed, registry, sample_rate, &typed.cafs)?;
    // regalloc::allocate(&mut ir);
    RillProgram::<T, 256>::new_with(ir, registry, sample_rate)
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
    resources: Option<&mut rill_core::buffer::ResourceRegistry<T>>,
) -> Result<program_engine::ProgramEngine<T, BUF>, CompileError> {
    let (program, resource_decls) = extract_resources(program);

    let mut typed = types::infer::infer_program_with(&program, registry)?;
    typed.program = reduce::reduce_with_cafs(&typed.program, &typed.cafs);
    let ir = lower::lower_with_cafs(&typed, registry, sample_rate, &typed.cafs)?;

    // The declared-resource check only applies when the registry is auto-created
    // from the source's `TapeLoop` declarations. When a caller-supplied registry
    // is used, its presence is validated below.
    if resources.is_none() {
        for bi in &ir.builtins {
            if let Some(res) = &bi.resource {
                if !resource_decls.iter().any(|d| &d.name == res) {
                    return Err(CompileError::Unsupported(format!(
                        "built-in '{}' references undeclared resource '{}'",
                        bi.name, res
                    )));
                }
            }
        }
    }

    let mut owned = rill_core::buffer::ResourceRegistry::<T>::new();
    let res: &mut rill_core::buffer::ResourceRegistry<T> = match resources {
        Some(r) => {
            for bi in &ir.builtins {
                if let Some(name) = &bi.resource {
                    if r.reader(name).is_none() {
                        return Err(CompileError::Unsupported(format!(
                            "resource '{}' not found in the provided registry",
                            name
                        )));
                    }
                }
            }
            r
        }
        None => {
            for decl in &resource_decls {
                let tape =
                    rill_core::buffer::TapeLoop::<T>::new(decl.capacity).ok_or_else(|| {
                        CompileError::Unsupported(format!("tape '{}' has zero capacity", decl.name))
                    })?;
                owned.register_buffer(decl.name.clone(), Box::new(tape));
            }
            &mut owned
        }
    };

    let rp = RillProgram::<T, BUF>::new_with_resources(ir, registry, sample_rate, res)?;
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

/// Extract top-level `name = TapeLoop <capacity>` resource declarations,
/// returning the remaining signal program plus the declarations.
fn extract_resources(program: &crate::ast::Program) -> (crate::ast::Program, Vec<ResourceDecl>) {
    use crate::ast::{Def, Expr};
    let mut decls = Vec::new();
    let mut defs = Vec::with_capacity(program.defs.len());
    for def in &program.defs {
        let mut is_resource = false;
        if let Def::Local {
            name,
            body: Expr::Apply {
                name: ctor, args, ..
            },
            ..
        } = def
        {
            if ctor == "TapeLoop" {
                if let Some(cap) = args.first().and_then(|a| match a {
                    Expr::Int(v, _) => Some(*v as usize),
                    Expr::Float(v, _) => Some(*v as usize),
                    _ => None,
                }) {
                    decls.push(ResourceDecl {
                        name: name.clone(),
                        capacity: cap,
                    });
                    is_resource = true;
                }
            }
        }
        if !is_resource {
            defs.push(def.clone());
        }
    }
    (crate::ast::Program { defs }, decls)
}

#[cfg(test)]
mod ir_tests {
    use super::*;
    use crate::builtin::{BuiltinKind, BuiltinSig, Registry};

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
    impl rill_core::builtin::BlockBuiltin<f32> for TestOsc {}

    fn sine_registry() -> Registry<f32> {
        let mut registry = Registry::<f32>::new();
        registry.register_block(
            BuiltinSig::simple("sine", 0, 1, 3, BuiltinKind::Block),
            |_, _| Box::new(TestOsc),
        );
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
        assert!(ir.num_inputs > 0);
        assert!(ir.num_outputs > 0);
    }
}
