use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

/// Run a pure value program over literal data and read back its single output
/// as a list of floats.
fn run_values(src: &str) -> Vec<f64> {
    let mut prog = compile::<f32>(src).unwrap();
    prog.run_value().unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::List { elems } => elems
            .iter()
            .map(|r| match prog.arena().get(*r).unwrap() {
                rill_lang::arena::Value::Float(f) => *f,
                other => panic!("expected a List of floats, got element {other:?}"),
            })
            .collect(),
        other => panic!("expected a List output, got {other:?}"),
    }
}

#[test]
fn dc_offset() {
    assert_eq!(
        run_values("main = map (\\x -> x + 1.0) [0.0, 1.0, 2.0]"),
        vec![1.0, 2.0, 3.0]
    );
}

#[test]
fn math_builtin_abs() {
    // `abs` is a buffer-algebra builtin: it is exercised over a constant -1.0
    // buffer (a zero-frequency square wave at amplitude -1).
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let mut prog =
        rill_lang::compile_with_ffi::<f32>("main = abs (square 0.0 (-1.0) 0.0)", &ffi, 44100.0)
            .unwrap();
    let mut out = [0.0f32; 4];
    prog.run_value().unwrap();
    assert!(prog.copy_output_samples(0, &mut out));
    assert_eq!(out, [1.0, 1.0, 1.0, 1.0]);
}

/// The `_` input wire is gone from the unary value model, so "fanout" is now
/// expressed as a definition applied to literal data: the where-defined
/// `scale` maps its transform over the list.
#[test]
fn application_in_fanout_lowers_correctly() {
    assert_eq!(
        run_values("main = scale 0.5 where { scale k = map (\\x -> x * k) [2.0, 4.0, 8.0]; }"),
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
