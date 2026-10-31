//! Shared world/eval helpers for the rill-lang integration tests.
//!
//! Each helper compiles a value expression with
//! [`compile_eval`](rill_lang::compile_eval), seats a fresh world for the
//! resulting IR (the foreign factory registry is owned by the world), and runs
//! the entry once with [`test_eval`](rill_lang::test_eval) — the value surface
//! these suites assert on. The world is seeded at 44.1 kHz, the rate the
//! pre-world `test_eval` surface used by default, so signal-aware expressions
//! keep the coefficients they were written against.
//!
//! A helper returns `Result<WorldState, CompileError>` rather than a bare
//! world: the suites assert both successful runs (`.unwrap()`) and rejected
//! sources (`.is_err()`), and the error type is the compile/run error the
//! caller already matches on.
//!
//! Each integration-test binary compiles this module whole, so a given suite
//! sees helpers it never calls; the module is therefore declared
//! `pub mod common;` in every test file, which keeps its items reachable (and
//! the test binaries free of dead-code warnings) without a lint allow.

use rill_lang::arena::Value;
use rill_lang::error::CompileError;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;
use rill_lang::program::WorldConfig;
use rill_lang::{compile_eval, test_eval};

pub use rill_lang::program::WorldState;

/// The default world sample rate for these suites.
pub const RATE: f32 = 44_100.0;

/// Compile `expr` against `defs` with an empty foreign registry, run it once in
/// a fresh `Strict`-mode world, and return that world.
pub fn eval(defs: &str, expr: &str) -> Result<WorldState<f32, 256>, CompileError> {
    eval_mode::<256>(defs, expr, ForeignRegistry::new(), Mode::Strict)
}

/// Like [`eval`], with the caller's foreign registry (moved into the world).
pub fn eval_ffi(
    defs: &str,
    expr: &str,
    ffi: ForeignRegistry<f32>,
) -> Result<WorldState<f32, 256>, CompileError> {
    eval_mode::<256>(defs, expr, ffi, Mode::Strict)
}

/// Like [`eval`], seeded at the caller's sample rate.
pub fn eval_at(
    defs: &str,
    expr: &str,
    ffi: ForeignRegistry<f32>,
    sample_rate: f32,
) -> Result<WorldState<f32, 256>, CompileError> {
    eval_mode_at::<256>(defs, expr, ffi, Mode::Strict, sample_rate)
}

/// Like [`eval_ffi`], under an explicit evaluation mode and block size.
pub fn eval_mode<const BUF: usize>(
    defs: &str,
    expr: &str,
    ffi: ForeignRegistry<f32>,
    mode: Mode,
) -> Result<WorldState<f32, BUF>, CompileError> {
    eval_mode_at::<BUF>(defs, expr, ffi, mode, RATE)
}

/// The common core: compile the value entry, seat a world for it with `ffi` and
/// `sample_rate` as its initial state, and run it once.
fn eval_mode_at<const BUF: usize>(
    defs: &str,
    expr: &str,
    ffi: ForeignRegistry<f32>,
    mode: Mode,
    sample_rate: f32,
) -> Result<WorldState<f32, BUF>, CompileError> {
    let ir = compile_eval::<f32, BUF>(defs, expr, mode)?;
    let mut world = WorldState::<f32, BUF>::new(WorldConfig {
        ffi: Some(ffi),
        params: rill_core::Params::new(sample_rate),
        ..Default::default()
    });
    test_eval(&ir, &mut world)?;
    Ok(world)
}

/// The run result value of `world` (a clone of the arena slot).
pub fn out_value(world: &WorldState<f32, 256>) -> Value {
    let r = world.result().expect("the program produced no result");
    world
        .arena()
        .get(r)
        .expect("the result ref must be live")
        .clone()
}

/// Copy the run result buffer into `dst`; `false` when the result is not a
/// whole-buffer value.
pub fn buffers(world: &WorldState<f32, 256>, dst: &mut [f32]) -> bool {
    match world.result() {
        Some(r) => world.copy_buffer(r, dst),
        None => false,
    }
}

/// An integral value as `i64`, whichever numeric track the compiler chose.
///
/// An unconstrained bounds literal defaults by `default (Integer, Double)`,
/// whose FIRST entry is `Integer` — so `listArray (Pair 0 2) xs` stores
/// `Integer` bounds, exactly as GHC 9.10.3 types
/// `listArray (0,2) [...]` as `Array Integer e` (measured). `Int` and
/// `Integer` are the SAME number on every bounds either can express, so a test
/// reading a bound must accept both tracks instead of pinning one.
pub fn integral(v: &Value) -> i64 {
    match v {
        Value::Int(i) => *i,
        Value::Integer(i) => i64::try_from(i).expect("a bound must fit an i64"),
        other => panic!("expected an integral bound, got {other:?}"),
    }
}
