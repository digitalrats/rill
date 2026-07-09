#![cfg(feature = "lang")]

use rill_adrift::lang_builtins::full_registry;
use rill_core::traits::Algorithm;
use rill_lang::compile_with;

#[test]
fn sample_builtin_inside_feedback_runs() {
    let reg = full_registry::<f32>();
    let mut prog =
        compile_with::<f32>("main = + ~ (onepole 300.0 0.5 : (_ * 0.5))", &reg, 48_000.0).unwrap();
    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(&input), &mut out).unwrap();
    let energy: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(energy < 10.0, "unstable feedback (energy {energy})");
}

#[test]
fn chained_block_then_sample_runs() {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>(
        "main = _ : lowpass 2000.0 0.7 : moog 500.0 0.6",
        &reg,
        48_000.0,
    )
    .unwrap();
    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(&input), &mut out).unwrap();
    let energy: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(energy > 0.0, "signal completely silenced");
    assert!(energy < 1.0, "unstable output (energy {energy})");
}

#[test]
fn block_builtin_then_feedback_runs() {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>(
        "main = _ : lowpass 1000.0 0.7 : (+ ~ (_ * 0.5))",
        &reg,
        48_000.0,
    )
    .unwrap();
    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(&input), &mut out).unwrap();
    let energy: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(energy > 0.0, "signal completely silenced");
    assert!(energy < 5.0, "unstable output (energy {energy})");
}

#[test]
fn fanout_with_builtin_in_one_branch_runs() {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>(
        "main = _ <: (onepole 400.0 0.5 , _ * 0.5) :> + ",
        &reg,
        48_000.0,
    )
    .unwrap();
    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(&input), &mut out).unwrap();
    let energy: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(energy > 0.0, "signal completely silenced");
    assert!(energy < 1.0, "unstable output (energy {energy})");
}

#[test]
fn const_arithmetic_params_fold() {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>(
        "main = _ : lowpass (1000.0 * 2.0) (0.5 + 0.2)",
        &reg,
        48_000.0,
    )
    .unwrap();
    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(&input), &mut out).unwrap();
    let energy: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(energy > 0.0, "signal completely silenced");

    let mut prog2 = compile_with::<f32>("main = _ : lowpass 2000.0 0.7", &reg, 48_000.0).unwrap();
    let mut out2 = vec![0.0f32; input.len()];
    prog2.process(Some(&input), &mut out2).unwrap();
    for (i, (x, y)) in out.iter().zip(out2.iter()).enumerate() {
        assert!(
            (x - y).abs() < 1e-5,
            "const-arith param {}: folded {x} vs explicit {y}",
            i
        );
    }
}

#[test]
fn block_builtin_in_feedback_rejected() {
    let reg = full_registry::<f32>();
    let err = compile_with::<f32>("main = + ~ lowpass 500.0 0.7", &reg, 48_000.0);
    assert!(err.is_err(), "block-in-feedback should be rejected");
}

#[test]
fn sample_builtins_hybrid_matches_reference() {
    let reg = full_registry::<f32>();
    let mut prog_hybrid = compile_with::<f32>(
        "main = _ : onepole 300.0 0.5 : moog 500.0 0.6",
        &reg,
        48_000.0,
    )
    .unwrap();
    let mut prog_ref = compile_with::<f32>(
        "main = _ : onepole 300.0 0.5 : moog 500.0 0.6",
        &reg,
        48_000.0,
    )
    .unwrap();
    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.1).sin()).collect();
    let mut out_h = vec![0.0f32; input.len()];
    let mut out_r = vec![0.0f32; input.len()];
    prog_hybrid.process(Some(&input), &mut out_h).unwrap();
    prog_ref
        .process_reference(Some(&input), &mut out_r)
        .unwrap();
    let max_diff = out_h
        .iter()
        .zip(out_r.iter())
        .map(|(h, r)| (h - r).abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_diff < 1e-3,
        "hybrid vs reference mismatch: max_diff={max_diff}"
    );
}

#[test]
fn integer_param_to_builtin_compiles() {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>("main = _ : lowpass 1000 0.7", &reg, 48_000.0).unwrap();
    let input: Vec<f32> = (0..32).map(|i| (i as f32 * 0.2).sin()).collect();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(&input), &mut out).unwrap();
    let energy: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(energy > 0.0, "int param program silenced output");
}
