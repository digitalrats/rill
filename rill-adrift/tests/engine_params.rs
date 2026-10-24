//! Parameter defaults on the retained value path.
//!
//! A `?name` control parameter is a slot the host writes through
//! [`WorldState::set_param`](rill_lang::program::WorldState::set_param).
//! The helpers in this file drive the single-run value surface: `test_eval`
//! seats a world and evaluates the entry exactly once, so only the
//! DEFAULT-reading behavior is pinned here. The `set_param`-between-runs tests
//! below were Class 2 when the value surface became single-run (there was no
//! host re-run surface then). `ProgramRunner::run()` is repeatable now, so the
//! runner path could re-express them; that is recorded test debt, not a gap in
//! the world API (ledger: docs/superpowers/ledgers/2026-10-18-world-state.md).

use rill_adrift::lang_builtins::full_ffi;
use rill_adrift::rill_core::Params;
use rill_lang::mode::Mode;
use rill_lang::program::{WorldConfig, WorldState};
use rill_lang::{compile_eval, test_eval};

const BUF: usize = 256;

/// The world sample rate this suite is pinned to (the pre-world `test_eval`
/// surface's explicit rate argument).
const RATE: f32 = 44_100.0;

/// Evaluate `src` once through the single-run value surface and return the
/// world, whose `result()` holds the run's output.
fn eval(src: &str) -> WorldState<f32, BUF> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ir = compile_eval::<f32, BUF>(defs, expr, Mode::Strict)
        .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"));
    let mut world = WorldState::<f32, BUF>::new(WorldConfig {
        ffi: Some(full_ffi::<f32>()),
        params: Params::new(RATE),
        ..Default::default()
    });
    test_eval(&ir, &mut world).unwrap_or_else(|e| panic!("program must run: {e}\nsource:\n{src}"));
    world
}

/// Read the single run's scalar output.
fn scalar_output(world: &WorldState<f32, BUF>) -> f64 {
    match world.arena().get(world.result().unwrap()).unwrap() {
        rill_lang::arena::Value::Float(v) => *v,
        other => panic!("expected a Float output, got {other:?}"),
    }
}

#[test]
fn actor_param_default_applies() {
    // `?gain=0.25` keeps its source default on the single run.
    let world = eval("main = (?gain=0.25) * 2.0");
    assert_eq!(scalar_output(&world), 0.5);
}

#[test]
fn actor_param_no_default_applies() {
    // A parameter with no source default starts at 0.0 on the single run.
    let world = eval("main = ?gain * 2.0");
    assert_eq!(scalar_output(&world), 0.0);
}

#[test]
fn actor_param_default_reaches_a_block_builtin() {
    // A parameter default reaches an FFI block builtin: `sine`'s amplitude,
    // whose `?level 0.0` default yields silence on the single run.
    let world = eval("main = sine 440.0 ?level 0.0");

    let r = world.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 64];
    assert!(world.copy_buffer(r, &mut out));
    assert_eq!(out, [0.0; 64], "level default is 0.0 => silence");
    // The live-parameter half (write `set_param(level, 0.5)`, re-run, assert
    // the amplitude) is not expressible through this single-run helper: `eval`
    // seats the world and runs `test_eval` once. The runner path (`compile` +
    // repeatable `ProgramRunner::run`) could support it; re-adding the test
    // there is recorded debt (see the module doc).
}

// Class 2 removals (ledger: docs/superpowers/ledgers/2026-10-18-world-state.md):
// - actor_param_set_param_affects_output :: sole assertion is value-from-set-param
//   between runs; no host re-run surface at removal time :: delete
//   (the default-application half survives as `actor_param_default_applies`)
// - actor_param_persists_across_ticks :: set_param + two value runs, cross-tick
//   persistence; no host re-run surface at removal time :: delete
// - actor_param_reaches_a_block_builtin :: set_param + re-run amplitude check;
//   reduced to the default-silence half (`actor_param_default_reaches_a_block_builtin`)
// Both deleted subjects are re-expressible on `ProgramRunner::run()` (repeatable
// since the world-state migration); restoring them is test debt, not an API gap.
