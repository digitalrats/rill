use float_cmp::approx_eq;
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
    assert_eq!(
        run("process = _ + 1;", &[0.0, 1.0, 2.0]),
        vec![1.0, 2.0, 3.0]
    );
}

#[test]
fn one_pole_lowpass_smoothing() {
    let out = run("process = + ~ (_ * 0.5);", &[1.0, 1.0, 1.0, 1.0]);
    assert!(approx_eq!(f32, out[0], 1.0, epsilon = 1e-6));
    assert!(approx_eq!(f32, out[1], 1.5, epsilon = 1e-6));
    assert!(approx_eq!(f32, out[2], 1.75, epsilon = 1e-6));
    assert!(approx_eq!(f32, out[3], 1.875, epsilon = 1e-6));
}

#[test]
fn math_builtin_abs() {
    assert_eq!(
        run("process = abs(_);", &[-2.0, 3.0, -4.0]),
        vec![2.0, 3.0, 4.0]
    );
}

#[test]
fn type_error_is_reported() {
    assert!(compile::<f32>("process = _ , _;").is_err());
}

#[test]
fn parse_error_is_reported() {
    assert!(compile::<f32>("process = _").is_err());
}

#[test]
fn missing_process_is_reported() {
    assert!(compile::<f32>("gain = _ * 0.5;").is_err());
}
