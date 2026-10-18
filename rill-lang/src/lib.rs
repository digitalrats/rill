//! # rill-lang
//!
//! A Faust-style functional streaming DSL that compiles to a runnable
//! [`RillProgram`]. A program executes exactly once to completion: running the
//! program's entry fragment captures its result into
//! [`RillProgram::result`]. Values are evaluated through
//! [`test_eval`](crate::test_eval) / [`compile`](crate::compile) — the
//! `main : IO ()` executor is [`ProgramRunner`]). See the crate guide for
//! language details.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod arena;
/// The signal-arrow core: combinators as arrow laws over [`arrow::ArrowTy`].
pub mod arrow;
pub mod ast;
pub mod backend;
/// Element-wise in-place buffer math as scalar loops (LLVM auto-vectorizes).
pub mod buffer_math;
pub mod builtin;
/// Built-in multi-IO signal processors (mixer, EQ, dry/wet).
pub mod builtins;
pub mod error;
/// FFI factory registry: foreign-declared builtins' Rust implementations.
pub mod ffi;
/// Graph IR formation: [`GraphSpec`](graph::GraphSpec) → the graph engine.
#[cfg(feature = "graph")]
pub mod graph;
pub mod ir;
pub mod lexer;
pub mod lower;
pub mod mode;
pub mod parser;
pub mod prelude;
pub mod program;
pub mod program_runner;
pub mod reduce;
pub mod register;
pub mod render;
pub mod serde_def;
pub mod types;

#[cfg(feature = "debug")]
pub mod debug;

/// `Program` ↔ JSON serialization contract.
#[cfg(feature = "serialization")]
pub mod program_serde;

pub use arena::{Arena, ArenaError, ArenaRef, ThunkState, Value, ValueKind};
pub use error::{CompileError, Span};
pub use ir::{Ir, ValueFunc, ValueInstr, ValueLayout};
pub use program::RillProgram;
pub use program_runner::{run_on_stack, ProgramRunner};
pub use serde_def::{compile_def, RillLangDef};

pub use builtin::{BuiltinKind, Registry};

use rill_core::math::Transcendental;

use crate::mode::Mode;

/// Compile rill-lang source into a runnable [`RillProgram`] for scalar type `T`.
///
/// Uses the runtime-safe default block size (`BUF = 256`). Compiling does not
/// execute the program; evaluate a value expression with
/// [`test_eval`] and inspect the run result with
/// [`RillProgram::result`].
///
/// ```
/// use rill_lang::compile;
///
/// let prog = compile::<f32>("main = 1.0 + 2.0").unwrap();
/// // `main` is the program's entry: its value is the single run result.
/// ```
pub fn compile<T: Transcendental>(src: &str) -> Result<RillProgram<T, 256>, CompileError> {
    compile_with_mode::<T>(src, Mode::Strict)
}

/// Compile rill-lang source under an explicit evaluation mode into a runnable
/// [`RillProgram`] for scalar type `T`.
///
/// The file's own `{-# MODE ... #-}` pragma overrides `mode`; the API key is
/// the default only when the file carries no pragma. Uses the runtime-safe
/// default block size (`BUF = 256`).
pub fn compile_with_mode<T: Transcendental>(
    src: &str,
    mode: Mode,
) -> Result<RillProgram<T, 256>, CompileError> {
    let ir = compile_to_ir(src, 44_100.0, mode)?;
    Ok(RillProgram::<T, 256>::new(ir))
}

/// Compile with a built-in registry and a sample rate. Uses the default block
/// size (`BUF = 256`).
pub fn compile_with<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<RillProgram<T, 256>, CompileError> {
    compile_with_mode_with(src, registry, sample_rate, Mode::Strict)
}

/// Like [`compile_with`], under an explicit evaluation mode (the file's
/// `{-# MODE ... #-}` pragma overrides the API key). Uses the default block
/// size (`BUF = 256`).
pub fn compile_with_mode_with<T: Transcendental>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
    mode: Mode,
) -> Result<RillProgram<T, 256>, CompileError> {
    let ir = compile_to_ir(src, sample_rate, mode)?;
    RillProgram::<T, 256>::new_with(ir, registry, sample_rate, None)
}

/// Compile source against a foreign registry: `foreign` builtins resolve
/// their Rust implementations from `ffi` (a fresh, empty legacy built-in
/// registry is used for the built-in path). Uses the default block size
/// (`BUF = 256`).
pub fn compile_with_ffi<T: Transcendental>(
    src: &str,
    ffi: &crate::ffi::ForeignRegistry<T>,
    sample_rate: f32,
) -> Result<RillProgram<T, 256>, CompileError> {
    compile_with_ffi_mode(src, ffi, sample_rate, Mode::Strict)
}

/// Like [`compile_with_ffi`], under an explicit evaluation mode (the file's
/// `{-# MODE ... #-}` pragma overrides the API key). Uses the default block
/// size (`BUF = 256`).
pub fn compile_with_ffi_mode<T: Transcendental>(
    src: &str,
    ffi: &crate::ffi::ForeignRegistry<T>,
    sample_rate: f32,
    mode: Mode,
) -> Result<RillProgram<T, 256>, CompileError> {
    let ir = compile_to_ir(src, sample_rate, mode)?;
    let registry = Registry::<T>::new();
    RillProgram::<T, 256>::new_with(ir, &registry, sample_rate, Some(ffi))
}

/// Evaluate a value expression against a set of definitions, returning the
/// already-run program.
///
/// This is the value-evaluation surface of rill-lang: `expr` is compiled as a
/// fresh entry definition in the context of `defs` and run once to completion.
/// The run result is captured into the returned program's
/// [`RillProgram::result`]; whole-buffer values are copied with
/// [`RillProgram::copy_buffer`].
///
/// Compilation is the SAME pipeline as [`compile`]: the source
/// `"{defs}\n__rill_eval = ({expr});"` is parsed, inferred and lowered with the
/// internal `__rill_eval` marker as the entry name. `main` is not special here:
/// a `main` binding inside `defs` is an ordinary value definition, so
/// `test_eval::<f32, 256>("main = 42.0", "main * 2.0", …)` evaluates to
/// `Float(84.0)`.
///
/// A run latching a [`ProcessError`](rill_core::traits::ProcessError) (a
/// collection/arena capacity overflow) surfaces as
/// `CompileError::Unsupported`.
///
/// ```
/// use rill_lang::{test_eval, ffi::ForeignRegistry};
///
/// let ffi = ForeignRegistry::<f32>::new();
/// let prog = test_eval::<f32, 256>("", "1.0 + 2.0", &ffi, 44_100.0).unwrap();
/// let r = prog.result().unwrap();
/// assert_eq!(
///     prog.arena().get(r).unwrap(),
///     &rill_lang::arena::Value::Float(3.0)
/// );
/// ```
pub fn test_eval<T: Transcendental, const BUF: usize>(
    defs: &str,
    expr: &str,
    ffi: &crate::ffi::ForeignRegistry<T>,
    sample_rate: f32,
) -> Result<RillProgram<T, BUF>, CompileError> {
    test_eval_with_mode::<T, BUF>(defs, expr, ffi, sample_rate, Mode::Strict)
}

/// Like [`test_eval`], under an explicit evaluation mode (`Strict` by default;
/// the file's `{-# MODE … #-}` pragma inside `defs` overrides the API key).
pub fn test_eval_with_mode<T: Transcendental, const BUF: usize>(
    defs: &str,
    expr: &str,
    ffi: &crate::ffi::ForeignRegistry<T>,
    sample_rate: f32,
    mode: Mode,
) -> Result<RillProgram<T, BUF>, CompileError> {
    // The internal entry marker is a valid identifier (`__rill_eval`); a defs
    // that defines the same name is a user error (last-wins — the appended
    // entry shadows it).
    const EVAL_ENTRY: &str = "__rill_eval";
    let src = format!("{defs}\n{EVAL_ENTRY} = ({expr});");
    let ir = compile_to_ir_entry(&src, sample_rate, mode, EVAL_ENTRY)?;
    let registry = Registry::<T>::new();
    let mut prog = RillProgram::<T, BUF>::new_with(ir, &registry, sample_rate, Some(ffi))?;
    crate::backend::interp::run_entry(&mut prog)
        .map_err(|e| CompileError::Unsupported(format!("eval run failed: {e}")))?;
    Ok(prog)
}

/// Compile rill-lang source into a [`ProgramRunner`] — the executor of
/// `main : (String…) -> IO ()`.
///
/// The runner's program is compiled in `Complete` (lazy) mode unless the source
/// carries its own `{-# MODE … #-}` pragma: the runner executes a
/// Turing-complete program, including recursive stream helpers that `Strict`
/// mode rejects.
///
/// The block size `BUF` bounds the buffer table; it does not affect the
/// program's value semantics. The runner takes no external I/O handles: the
/// program's streams are created by the program itself.
///
/// ```
/// use rill_lang::{compile_runner, Registry};
///
/// let mut runner = compile_runner::<f32, 256>(
///     r#"main = do { write (Just "hi") stdout; return () };"#,
///     &Registry::new(),
///     44_100.0,
/// )
/// .unwrap();
/// runner.run().unwrap();
/// assert_eq!(runner.stdout(), ["hi"]);
/// ```
pub fn compile_runner<T: Transcendental, const BUF: usize>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
) -> Result<ProgramRunner<T, BUF>, CompileError> {
    let program = compile_source_to_program::<T, BUF>(src, registry, sample_rate, Mode::Complete)?;
    Ok(ProgramRunner::new(program))
}

/// The source-to-program pipeline shared by the executable (runner) entry
/// points: tokenize → parse → resource extraction → mode resolution → infer →
/// reduce → lower → program assembly.
fn compile_source_to_program<T: Transcendental, const BUF: usize>(
    src: &str,
    registry: &Registry<T>,
    sample_rate: f32,
    mode: Mode,
) -> Result<RillProgram<T, BUF>, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let (mut program, tape_decls) = extract_resources(&program)?;
    apply_api_mode(&mut program, mode);
    let mut typed = types::infer::infer_program_with(&program, program.mode)?;
    typed.tape_decls = tape_decls
        .iter()
        .map(|d| (d.name.clone(), d.capacity))
        .collect();
    typed.program = reduce::reduce_with_cafs_mode(&typed.program, &typed.cafs, typed.mode);
    let ir = lower::lower_program_with_channels(
        &typed,
        sample_rate,
        &typed.cafs,
        typed.mode,
        0,
        None,
        "main",
    )?;
    // No external resource registry is supplied: every resource-backed built-in
    // must resolve to a `tape_loop` declaration in the source.
    for bi in &ir.builtins {
        if let Some(res) = &bi.resource {
            if !tape_decls.iter().any(|d| &d.name == res) {
                return Err(CompileError::Unsupported(format!(
                    "built-in '{}' references undeclared resource '{}' \
                     (declare it with `tape = tape_loop <capacity>`)",
                    bi.name, res
                )));
            }
        }
    }
    RillProgram::<T, BUF>::new_with(ir, registry, sample_rate, None)
}

/// Resolve the effective evaluation mode for a parsed program: the file's
/// `{-# MODE ... #-}` pragma wins; the API-supplied mode is the default only
/// when the file carries no pragma.
fn apply_api_mode(program: &mut crate::ast::Program, api_mode: Mode) {
    if !program.mode_explicit {
        program.mode = api_mode;
    }
}

/// The shared source-compile pipeline: tokenize → parse → resource extraction
/// → mode resolution → infer → reduce → lower. The effective mode (pragma or
/// API key) is threaded onto the [`TypedProgram`](types::infer::TypedProgram).
///
/// `entry` names the definition whose value is the program's single result
/// (compile entries use `"main"`; [`test_eval`] uses an internal marker).
fn compile_to_ir(src: &str, sample_rate: f32, mode: Mode) -> Result<Ir, CompileError> {
    compile_to_ir_entry(src, sample_rate, mode, "main")
}

/// Like [`compile_to_ir`], with an explicit entry name.
///
/// The entry def supplies the program's process type (inference) and its value
/// result (lowering): the whole pipeline treats `main` and `test_eval`'s
/// internal marker identically. `main` is used for the runner path; the marker
/// never triggers infer's literal-`main` special cases (String launch args, the
/// `IO ()` retry), so a value expression evaluates as a plain binding.
fn compile_to_ir_entry(
    src: &str,
    sample_rate: f32,
    mode: Mode,
    entry: &str,
) -> Result<Ir, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let (mut program, tape_decls) = extract_resources(&program)?;
    apply_api_mode(&mut program, mode);
    let mut typed = types::infer::infer_program_with_entry(&program, program.mode, entry)?;
    typed.tape_decls = tape_decls
        .iter()
        .map(|d| (d.name.clone(), d.capacity))
        .collect();
    typed.program = reduce::reduce_with_cafs_mode(&typed.program, &typed.cafs, typed.mode);
    lower::lower_with_cafs_mode_entry(&typed, sample_rate, &typed.cafs, typed.mode, entry)
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
        if let Def::Eval {
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
    Ok((
        crate::ast::Program {
            defs,
            mode: program.mode,
            mode_explicit: program.mode_explicit,
        },
        decls,
    ))
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
        registry.register_block("sine", |_, _, _alloc| Box::new(TestOsc));
        registry
    }

    #[test]
    fn public_compile_rejects_recursive_caf() {
        // a = a -> graceful CompileError, not stack overflow
        let res = compile::<f32>("a = a; main = a");
        assert!(res.is_err());
    }

    #[test]
    fn public_compile_with_closed_caf_compiles() {
        // A closed CAF (`osc`, 0 signal inputs) referenced from the value model
        // compiles and instantiates its generator builtin. Sharing one closed
        // CAF across multiple references is a Thunk-model concern (Task 7);
        // the value track lowers each reference to its own instance.
        let registry = sine_registry();
        let prog = compile_with::<f32>(
            "osc = sine 440.0 1.0 0.0; main = osc * 1.0;",
            &registry,
            44100.0,
        )
        .unwrap();
        let sines = prog.ir.builtins.iter().filter(|b| b.name == "sine").count();
        assert_eq!(sines, 1);
    }

    #[test]
    fn lang_chiptune_ir_structure() {
        let mut registry = Registry::<f32>::new();
        registry.register_block("ay38910", |_, _, _alloc| panic!("not instantiated"));
        // lofi: 1 signal in (from pipeline :), 1 out, 7 params
        registry.register_block("lofi", |_, _, _alloc| panic!("not instantiated"));

        let src = r"main = lofi 8 44100 0.75 1.0 1 0 1 (ay38910 1750000.0 1.0);";
        let tokens = lexer::tokenize(src).unwrap();
        let program = parser::parse(&tokens, src.as_bytes()).unwrap();
        let mut typed = types::infer::infer_program_with(&program, program.mode).unwrap();
        typed.program = reduce::reduce(&typed.program);
        let ir = lower::lower_with(&typed, 44100.0).unwrap();
        assert!(
            ir.num_outputs > 0,
            "the signal program must produce an output channel"
        );
        // Each block call is instantiated in a callable fragment of the value
        // track (the block arguments are curried closures), so the foreign
        // call lives in one of the program's fragments rather than the main
        // value block.
        assert!(
            ir.value_blocks
                .iter()
                .chain(ir.fragments.iter().flat_map(|f| &f.value_blocks))
                .flat_map(|b| &b.instrs)
                .any(|i| matches!(i, ir::ValueInstr::ValueCallForeign { .. })),
            "the block calls must instantiate their foreign builtins"
        );
        assert_eq!(
            ir.builtins
                .iter()
                .map(|b| b.name.as_str())
                .collect::<Vec<_>>(),
            vec!["ay38910", "lofi"]
        );
    }

    #[test]
    fn complete_mode_is_carried_to_typed_program() {
        let tokens = lexer::tokenize("{-# MODE complete #-}\nmain = 1.0").unwrap();
        let program =
            parser::parse(&tokens, "{-# MODE complete #-}\nmain = 1.0".as_bytes()).unwrap();
        let typed = types::infer::infer_program(&program, crate::mode::Mode::Complete).unwrap();
        assert_eq!(typed.mode, crate::mode::Mode::Complete);
    }

    #[test]
    fn api_mode_is_default_when_no_pragma() {
        let tokens = lexer::tokenize("main = 1.0").unwrap();
        let mut program = parser::parse(&tokens, "main = 1.0".as_bytes()).unwrap();
        apply_api_mode(&mut program, crate::mode::Mode::Complete);
        assert_eq!(program.mode, crate::mode::Mode::Complete);
        assert!(!program.mode_explicit);
    }

    #[test]
    fn pragma_wins_over_api_mode() {
        let tokens = lexer::tokenize("{-# MODE complete #-}\nmain = 1.0").unwrap();
        let mut program =
            parser::parse(&tokens, "{-# MODE complete #-}\nmain = 1.0".as_bytes()).unwrap();
        assert!(program.mode_explicit);
        apply_api_mode(&mut program, crate::mode::Mode::Strict);
        assert_eq!(program.mode, crate::mode::Mode::Complete);
    }

    #[test]
    fn compile_with_mode_complete_compiles() {
        let res = compile_with_mode::<f32>("main = 1.0", crate::mode::Mode::Complete);
        assert!(res.is_ok());
    }

    #[test]
    fn pragma_flows_through_public_compile() {
        let res = compile::<f32>("{-# MODE complete #-}\nmain = 1.0");
        assert!(res.is_ok());
    }
}
