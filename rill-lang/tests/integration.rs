use rill_core::traits::Algorithm;
use rill_lang::compile;

fn run(src: &str, input: &[f32]) -> Vec<f32> {
    let mut prog = compile::<f32>(src).unwrap();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(input), &mut out).unwrap();
    out
}

#[test]
fn dc_offset() {
    assert_eq!(run("main = _ + 1", &[0.0, 1.0, 2.0]), vec![1.0, 2.0, 3.0]);
}

#[test]
fn feedback_tap_uses_previous_tick() {
    // `+ ~ _`: out[i] = x[i] + fb[i], where fb is the previous tick's input.
    let mut prog = compile::<f32>("main = + ~ _").unwrap();
    let mut out = vec![0.0f32; 4];
    prog.process(Some(&[1.0, 2.0, 3.0, 4.0]), &mut out).unwrap();
    assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0]);
    prog.process(Some(&[5.0, 6.0, 7.0, 8.0]), &mut out).unwrap();
    assert_eq!(out, vec![6.0, 8.0, 10.0, 12.0]);
}

#[test]
fn math_builtin_abs() {
    assert_eq!(run("main = abs _", &[-2.0, 3.0, -4.0]), vec![2.0, 3.0, 4.0]);
}

#[test]
fn application_in_fanout_lowers_correctly() {
    assert_eq!(
        run("main = g 0.5 where { g x = _ * x; }", &[2.0, 4.0, 8.0]),
        vec![1.0, 2.0, 4.0]
    );
}

// Legacy application test — keep-compiled

#[test]
fn type_error_is_reported() {
    assert!(compile::<f32>("main = !").is_err());
}

#[test]
fn parse_error_is_reported() {
    assert!(compile::<f32>("_").is_err());
}

#[test]
fn plain_expression_rejected() {
    assert!(compile::<f32>("_ * 0.5").is_err());
}
