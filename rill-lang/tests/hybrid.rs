//! Hybrid backend: equivalence with the reference interpreter + behavior.

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
    let input: Vec<f32> = (0..64).map(|i| ((i as f32) * 0.13).sin() * 0.7).collect();
    let h = hybrid(src, &input);
    let r = reference(src, &input);
    for (k, (x, y)) in h.iter().zip(r.iter()).enumerate() {
        assert!((x - y).abs() < 1e-4, "[{k}] {src}: hybrid {x} vs ref {y}");
    }
}

#[test]
fn equiv_feedforward() {
    assert_equiv("process = _ * 0.5;");
    assert_equiv("process = abs(_) : _ * 2.0;");
    assert_equiv("process = _ + 1.0 : sin;");
    assert_equiv("process = _ <: (_ , _ * 0.5) :> +;");
}

#[test]
fn equiv_feedback() {
    assert_equiv("process = + ~ _;");
    assert_equiv("process = + ~ (_ * 0.5);");
    assert_equiv("process = + ~ (_ * 0.9) : _ * 0.1;");
}

#[test]
fn equiv_delay_and_mixed() {
    assert_equiv("process = _ @ 1;");
    assert_equiv("process = _ @ 5;");
    assert_equiv("process = (_ * 0.5) : (+ ~ (_ @ 2));");
    assert_equiv("process = (_ @ 3) : (+ ~ _);");
}

#[test]
fn exact_values_hold() {
    assert_eq!(
        hybrid("process = _ * 0.5;", &[1.0, 2.0, 4.0, 8.0]),
        vec![0.5, 1.0, 2.0, 4.0]
    );
    assert_eq!(
        hybrid("process = + ~ _;", &[1.0, 1.0, 1.0, 1.0]),
        vec![1.0, 2.0, 3.0, 4.0]
    );
    assert_eq!(
        hybrid("process = _ @ 1;", &[5.0, 7.0, 9.0]),
        vec![0.0, 5.0, 7.0]
    );
}

#[test]
fn multi_block_state_persists() {
    // Two consecutive calls: the integrator state must carry across blocks.
    let mut prog = compile::<f32>("process = + ~ _;").unwrap();
    let mut o1 = [0.0f32; 3];
    let mut o2 = [0.0f32; 3];
    prog.process(Some(&[1.0, 1.0, 1.0]), &mut o1).unwrap();
    prog.process(Some(&[1.0, 1.0, 1.0]), &mut o2).unwrap();
    assert_eq!(o1, [1.0, 2.0, 3.0]);
    assert_eq!(o2, [4.0, 5.0, 6.0]);
}

#[test]
fn varying_block_length_reuses_store() {
    // Larger then smaller blocks must both work (store grows once, then reused).
    let mut prog = compile::<f32>("process = _ * 2;").unwrap();
    let mut big = vec![0.0f32; 100];
    prog.process(Some(&vec![1.0f32; 100]), &mut big).unwrap();
    assert!(big.iter().all(|&v| v == 2.0));
    let mut small = [0.0f32; 4];
    prog.process(Some(&[3.0, 3.0, 3.0, 3.0]), &mut small)
        .unwrap();
    assert_eq!(small, [6.0, 6.0, 6.0, 6.0]);
}
