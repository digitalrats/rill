//! Stress-tests comparing hybrid block/sample executor against the reference
//! per-sample oracle for complex multi-region programs.

use rill_core::traits::Algorithm;
use rill_lang::compile;

fn hybrid(src: &str, input: &[f32]) -> Vec<f32> {
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(input), &mut out).unwrap();
    out
}

fn reference(src: &str, input: &[f32]) -> Vec<f32> {
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = vec![0.0f32; input.len()];
    prog.process_reference(Some(input), &mut out).unwrap();
    out
}

fn assert_equiv(src: &str) {
    let input: Vec<f32> = (0..128)
        .map(|i| {
            let t = i as f32 * 0.07;
            0.5 * (t.sin() + (t * 1.7).sin() * 0.6)
        })
        .collect();
    let h = hybrid(src, &input);
    let r = reference(src, &input);
    for (k, (x, y)) in h.iter().zip(r.iter()).enumerate() {
        assert!((x - y).abs() < 1e-3, "[{k}] {src}: hybrid {x} vs ref {y}");
    }
}

#[test]
fn stress_parallel_feedbacks_merged() {
    assert_equiv("process = _ <: (+ ~ (_ * 0.5)) , (+ ~ (_ * 0.9)) :> +;");
}

#[test]
fn stress_feedback_feeding_feedforward() {
    assert_equiv("process = (+ ~ (_ * 0.8)) : abs : (_ * 0.5);");
}

#[test]
fn stress_delay_inside_outside_feedback() {
    assert_equiv("process = (_ @ 2) : (+ ~ (_ @ 1));");
}

#[test]
fn stress_fanout_mixed_branches_feedback() {
    assert_equiv("process = _ <: (_ , _ * 0.5) :> (+ ~ (_ * 0.7));");
}

#[test]
fn stress_nested_feedback_composition() {
    assert_equiv("process = (+ ~ _) : (+ ~ (_ * 0.5));");
}

#[test]
fn stress_deep_feedforward_chain() {
    assert_equiv("process = _ * 0.5 : abs : sqrt : (_ * 0.5);");
}

#[test]
fn stress_multi_block_state_carry() {
    let mut prog =
        compile::<f32>("process = _ <: (+ ~ (_ * 0.5)) , (+ ~ (_ * 0.9)) :> +;").unwrap();
    let mut out1 = vec![0.0f32; 128];
    let mut out2 = vec![0.0f32; 128];
    let input1: Vec<f32> = (0..128).map(|i| (i as f32 * 0.07).sin() * 0.8).collect();
    let input2: Vec<f32> = (0..128)
        .map(|i| ((i + 128) as f32 * 0.07).sin() * 0.8)
        .collect();
    prog.process(Some(&input1), &mut out1).unwrap();
    prog.process(Some(&input2), &mut out2).unwrap();

    let mut prog_ref =
        compile::<f32>("process = _ <: (+ ~ (_ * 0.5)) , (+ ~ (_ * 0.9)) :> +;").unwrap();
    let mut ref1 = vec![0.0f32; 128];
    let mut ref2 = vec![0.0f32; 128];
    prog_ref
        .process_reference(Some(&input1), &mut ref1)
        .unwrap();
    prog_ref
        .process_reference(Some(&input2), &mut ref2)
        .unwrap();

    for (k, (h, r)) in out1.iter().zip(ref1.iter()).enumerate() {
        assert!((h - r).abs() < 1e-3, "block1[{k}]: hybrid {h} vs ref {r}");
    }
    for (k, (h, r)) in out2.iter().zip(ref2.iter()).enumerate() {
        assert!((h - r).abs() < 1e-3, "block2[{k}]: hybrid {h} vs ref {r}");
    }
}
