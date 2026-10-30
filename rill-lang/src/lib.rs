//! # rill-lang
//!
//! A Faust-style functional streaming DSL. Compilation is **explicit**: a
//! source program lowers to a pure-code [`Ir`], and a [`WorldState`] holds every
//! piece of runtime state. A world is seated for an IR
//! ([`WorldState::seat`]) and executed by the `(ir, world)` executor.
//!
//! Values are evaluated with [`compile_eval`] + [`test_eval`]; the
//! `main : IO ()` executor is [`compile`] → [`ProgramRunner`]. Runs are
//! repeatable: each [`test_eval`] or [`ProgramRunner::run`] re-seats the world
//! and captures the run's terminal value into [`WorldState::result`], so a
//! `set_param` between runs is observed by the next one. See the crate guide
//! for language details.

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
/// The single desugaring pass: the parser's equation surface (guards,
/// argument patterns, `where`, standalone signatures) to core AST nodes.
pub mod desugar;
pub mod error;
/// FFI factory registry: foreign-declared builtins' Rust implementations.
pub mod ffi;
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

/// Dictionary planning: the `(class, type)` pairs a program needs and their
/// build order (design §3.7 step 2).
pub mod dicts;

/// The one `(class, method) → kernel` table (design §3.9), shared by lowering
/// and dictionary construction.
pub mod kernels;

/// `Program` ↔ JSON serialization contract.
#[cfg(feature = "serialization")]
pub mod program_serde;

pub use arena::{Arena, ArenaError, ArenaRef, DictSlot, KernelId, ThunkState, Value, ValueKind};
pub use error::{CompileError, Span};
pub use ir::{Ir, ValueFunc, ValueInstr, ValueLayout, ValueTyTag};
pub use program::{WorldConfig, WorldState};
pub use program_runner::{run_on_stack, ProgramRunner};
pub use serde_def::{compile_def, RillLangDef};

pub use builtin::BuiltinKind;

use rill_core::math::Transcendental;

use crate::mode::Mode;

/// Compile `main : IO ()` source into a runnable [`ProgramRunner`] for scalar
/// type `T`.
///
/// Compilation is in `Complete` (lazy) mode unless the source carries its own
/// `{-# MODE … #-}` pragma: the runner executes a Turing-complete program,
/// including recursive stream helpers that `Strict` mode rejects. Compiling
/// does not execute the program; [`ProgramRunner::run`] does.
///
/// The registry is **owned by the world** the runner holds: `ffi` resolves
/// every `foreign`-declared built-in at seat time. The block size `BUF` bounds
/// the buffer table; it does not affect the program's value semantics.
///
/// `main` is a pure `IO ()` value, not a function: a source whose entry does
/// not type as `IO ()` is rejected here, before any world exists.
///
/// ```
/// use rill_lang::{compile, ffi::ForeignRegistry};
///
/// let mut runner = compile::<f32, 256>(
///     r#"main = do { write (Just "hi") stdout; return () };"#,
///     ForeignRegistry::new(),
/// )
/// .unwrap();
/// runner.run().unwrap();
/// assert_eq!(runner.stdout(), ["hi"]);
/// ```
pub fn compile<T: Transcendental, const BUF: usize>(
    src: &str,
    ffi: crate::ffi::ForeignRegistry<T>,
) -> Result<ProgramRunner<T, BUF>, CompileError> {
    let ir = compile_to_ir(src, Mode::Complete)?;
    if !ir.main_io_unit {
        return Err(CompileError::Unsupported(
            "`main` must have type `IO ()`".to_string(),
        ));
    }
    // A name-based resource binding (`bi.resource = Some(name)`) is a `Ref` to a
    // tape the source never declared (`tape = tape_loop <capacity>`): report it
    // on the source path instead of letting it surface as a seat-time
    // "carries no tape index" fallthrough.
    for bi in &ir.builtins {
        if let Some(res) = &bi.resource {
            if bi.tape_index.is_none() {
                return Err(CompileError::Unsupported(format!(
                    "built-in '{}' references undeclared resource '{}' \
                     (declare it with `tape = tape_loop <capacity>`)",
                    bi.name, res
                )));
            }
        }
    }
    let world = WorldState::<T, BUF>::new(WorldConfig {
        ffi: Some(ffi),
        ..Default::default()
    });
    Ok(ProgramRunner::new(ir, world))
}

/// Compile a value expression against a set of definitions into an [`Ir`].
///
/// This is the compile half of the value-evaluation surface: `expr` is compiled
/// as a fresh entry definition in the context of `defs`. The compiled IR is
/// then run against a caller-owned world with [`test_eval`].
///
/// Compilation is the SAME pipeline as [`compile`]: the source
/// `"{defs}\n__rill_eval = ({expr});"` is parsed, inferred and lowered with the
/// internal `__rill_eval` marker as the entry name. `main` is not special here:
/// a `main` binding inside `defs` is an ordinary value definition, so
/// `compile_eval::<f32, 256>("main = 42.0", "main * 2.0", …)` evaluates to
/// `Double(84.0)`.
///
/// The scalar type `T` and block size `BUF` name the world the expression is
/// meant to be run in; the emitted IR is scalar-type and block-size agnostic.
pub fn compile_eval<T: Transcendental, const BUF: usize>(
    defs: &str,
    expr: &str,
    mode: Mode,
) -> Result<Ir, CompileError> {
    // The internal entry marker is a valid identifier (`__rill_eval`); a defs
    // that defines the same name is a user error (last-wins — the appended
    // entry shadows it).
    const EVAL_ENTRY: &str = "__rill_eval";
    let src = format!("{defs}\n{EVAL_ENTRY} = ({expr});");
    compile_to_ir_entry(&src, mode, EVAL_ENTRY)
}

/// Seat `world` for `ir` and execute the entry fragment once.
///
/// The world assembles its structural tables from the IR's exact layout
/// ([`WorldState::seat`]), resolving every built-in against the registry the
/// world owns and seeding the declared parameter defaults where the world's bag
/// has no value yet. Execution then walks the program's top-level value track
/// once, capturing the result into [`WorldState::result`]; whole-buffer values
/// are copied with [`WorldState::copy_buffer`].
///
/// A run latching a [`ProcessError`](rill_core::traits::ProcessError) (a
/// collection/arena capacity overflow, or a malformed `IO` program) surfaces as
/// `CompileError::Unsupported`.
///
/// ```
/// use rill_lang::{compile_eval, test_eval, ffi::ForeignRegistry, mode::Mode};
/// use rill_lang::program::{WorldConfig, WorldState};
///
/// let ir = compile_eval::<f32, 256>("", "1.0 + 2.0", Mode::Strict).unwrap();
/// let mut world = WorldState::<f32, 256>::new(WorldConfig {
///     ffi: Some(ForeignRegistry::new()),
///     ..Default::default()
/// });
/// test_eval(&ir, &mut world).unwrap();
/// let r = world.result().unwrap();
/// assert_eq!(
///     world.arena().get(r).unwrap(),
///     &rill_lang::arena::Value::Double(3.0)
/// );
/// ```
pub fn test_eval<T: Transcendental, const BUF: usize>(
    ir: &Ir,
    world: &mut WorldState<T, BUF>,
) -> Result<(), CompileError> {
    world.seat(ir)?;
    crate::backend::interp::exec_entry(ir, world)
        .map_err(|e| CompileError::Unsupported(format!("eval run failed: {e}")))
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
/// → mode resolution → desugar → infer → reduce → lower. The effective mode
/// (pragma or API key) is threaded onto the
/// [`TypedProgram`](types::infer::TypedProgram).
///
/// `entry` names the definition whose value is the program's single result
/// (the executable path uses `"main"`; [`compile_eval`] uses an internal
/// marker).
fn compile_to_ir(src: &str, mode: Mode) -> Result<Ir, CompileError> {
    compile_to_ir_entry(src, mode, "main")
}

/// Like [`compile_to_ir`], with an explicit entry name.
///
/// The entry def supplies the program's value result (lowering): the whole
/// pipeline treats `main` and [`compile_eval`]'s internal marker identically.
/// `main` is used for the executable path; the marker evaluates a plain value
/// expression as an ordinary binding.
fn compile_to_ir_entry(src: &str, mode: Mode, entry: &str) -> Result<Ir, CompileError> {
    let tokens = lexer::tokenize(src)?;
    let program = parser::parse(&tokens, src.as_bytes())?;
    let (mut program, tape_decls) = extract_resources(&program)?;
    apply_api_mode(&mut program, mode);
    // The ONE place the parser's equation surface becomes core AST nodes:
    // guard alternatives and equation sets become `Expr::Match`, argument
    // patterns a chain of matches over the formal parameters, `where` an
    // `Expr::Let` around the body, and standalone signatures are attached to
    // their bindings. Inference and lowering therefore never see the surface
    // forms — System FC makes `case` the only branching form, so the type
    // checker needs no knowledge of them.
    desugar::desugar_program(&mut program)?;
    let mut typed = types::infer::infer_program_with_entry(&program, program.mode, entry)?;
    typed.tape_decls = tape_decls
        .iter()
        .map(|d| (d.name.clone(), d.capacity))
        .collect();
    typed.program = reduce::reduce_with_cafs_mode(&typed.program, &typed.cafs, typed.mode);
    lower::lower_with_cafs_mode_entry(&typed, &typed.cafs, typed.mode, entry)
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
/// The `TapeLoop` spelling is accepted as a legacy alias for `tape_loop`; the
/// DSL path uses `tape_loop`.
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
                        let cap = crate::lower::literal_to_i64(v) as usize;
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

    #[test]
    fn public_compile_rejects_recursive_caf() {
        // a = a -> graceful CompileError, not stack overflow
        let res = compile_to_ir("a = a; main = a", crate::mode::Mode::Strict);
        assert!(res.is_err());
    }

    #[test]
    fn closed_caf_compiles() {
        // A closed CAF (`osc`, 0 signal inputs) referenced from the value model
        // compiles and instantiates its generator builtin. Sharing one closed
        // CAF across multiple references is a Thunk-model concern (Task 7);
        // the value track lowers each reference to its own instance.
        let ir = compile_to_ir(
            "osc = sine 440.0 1.0 0.0; main = osc * 1.0;",
            crate::mode::Mode::Strict,
        )
        .unwrap();
        let sines = ir.builtins.iter().filter(|b| b.name == "sine").count();
        assert_eq!(sines, 1);
    }

    #[test]
    fn lang_chiptune_ir_structure() {
        let src = r"main = lofi 8 44100 0.75 1.0 1 0 1 (ay38910 1750000.0 1.0);";
        let tokens = lexer::tokenize(src).unwrap();
        let program = parser::parse(&tokens, src.as_bytes()).unwrap();
        let mut typed = types::infer::infer_program_with(&program, program.mode).unwrap();
        typed.program = reduce::reduce(&typed.program);
        let ir = lower::lower_with(&typed).unwrap();
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
    fn complete_mode_compiles() {
        let res = compile_to_ir("main = 1.0", crate::mode::Mode::Complete);
        assert!(res.is_ok());
    }

    #[test]
    fn pragma_flows_through_compile_pipeline() {
        let res = compile_to_ir(
            "{-# MODE complete #-}\nmain = 1.0",
            crate::mode::Mode::Strict,
        );
        assert!(res.is_ok());
    }
}
