//! rill-lang builtin chains and parameter folding on the retained value path.

use rill_adrift::lang_builtins::full_ffi;
use rill_adrift::rill_core::Params;
use rill_lang::mode::Mode;
use rill_lang::program::{WorldConfig, WorldState};
use rill_lang::{compile_eval, test_eval};

const BUF: usize = 256;

/// The world sample rate this suite is pinned to (the pre-world `test_eval`
/// surface's explicit rate argument).
const RATE: f32 = 48_000.0;

/// Evaluate `src` against rill-adrift's FFI registry and return the single
/// whole-buffer result: the expression is compiled by `compile_eval`, seated
/// and run once by `test_eval` in a world that owns the registry.
fn run(src: &str) -> Vec<f32> {
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
    let r = world.result().expect("whole-buffer result expected");
    let mut out = vec![0.0f32; BUF];
    assert!(
        world.copy_buffer(r, &mut out),
        "program must produce a buffer output"
    );
    out
}

fn energy(samples: &[f32]) -> f32 {
    samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32
}

#[test]
fn chained_blocks_runs() {
    let out = run("main = moog 500.0 0.6 (lowpass 2000.0 0.7 (square 440.0 1.0 0.0))");
    let e = energy(&out);
    assert!(e > 0.0, "signal completely silenced");
    assert!(e < 1.0, "unstable output (energy {e})");
}

#[test]
fn block_chain_keeps_the_signal_bounded() {
    let out = run("main = onepole 1000.0 0.7 (lowpass 500.0 0.9 (square 440.0 1.0 0.0))");
    let e = energy(&out);
    assert!(e > 0.0, "signal completely silenced");
    assert!(e < 5.0, "unstable output (energy {e})");
}

#[test]
fn const_arithmetic_params_fold() {
    let folded = run("main = lowpass (1000.0 * 2.0) (0.5 + 0.2) (square 0.0 1.0 0.0)");
    let explicit = run("main = lowpass 2000.0 0.7 (square 0.0 1.0 0.0)");
    for (i, (x, y)) in folded.iter().zip(explicit.iter()).enumerate() {
        assert!(
            (x - y).abs() < 1e-5,
            "const-arith param {i}: folded {x} vs explicit {y}"
        );
    }
}

#[test]
fn integer_param_to_builtin_compiles() {
    let out = run("main = lowpass 1000 0.7 (square 0.0 1.0 0.0)");
    assert!(energy(&out) > 0.0, "int param program silenced output");
}
