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
        run(
            "main = do { writeOutput (map (\\x -> x + 1.0) (readInput)); return () }",
            &[0.0, 1.0, 2.0]
        ),
        vec![1.0, 2.0, 3.0]
    );
}

#[test]
fn math_builtin_abs() {
    assert_eq!(
        run(
            "main = do { writeOutput (map (\\x -> abs x) (readInput)); return () }",
            &[-2.0, 3.0, -4.0]
        ),
        vec![2.0, 3.0, 4.0]
    );
}

/// The `_` input wire is gone from the unary value model, so "fanout" is now
/// expressed as a definition that reads the input itself: the where-defined
/// `scale` is applied to a scalar and maps the transform over the input
/// channels through `readInput`.
#[test]
fn application_in_fanout_lowers_correctly() {
    assert_eq!(
        run(
            "main = scale 0.5 where { scale k = do { writeOutput (map (\\x -> x * k) (readInput)); return () }; }",
            &[2.0, 4.0, 8.0],
        ),
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
