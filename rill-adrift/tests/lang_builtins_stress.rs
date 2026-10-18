//! rill-lang builtin chains and parameter folding on the retained value path.

use rill_adrift::lang_builtins::full_ffi;
use rill_lang::test_eval;

const BUF: usize = 256;

/// Evaluate `src` against rill-adrift's FFI registry and return the single
/// whole-buffer result (`test_eval` compiles `main = <expr>` and runs it once).
fn run(src: &str) -> Vec<f32> {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = full_ffi::<f32>();
    let prog = test_eval::<f32, 256>(defs, expr, &ffi, 48_000.0)
        .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"));
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = vec![0.0f32; BUF];
    assert!(
        prog.copy_buffer(r, &mut out),
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
