//! Parameter routing on the retained value path.
//!
//! A `?name` control parameter is a slot the host writes through
//! [`RillProgram::set_param`](rill_lang::program::RillProgram::set_param); the
//! next tick (or value run) reads it. Defaults apply until the host writes.

use rill_adrift::lang_builtins::full_ffi;
use rill_core::traits::ParamValue;
use rill_lang::arena::Value;
use rill_lang::compile_with_ffi;
use rill_lang::program::RillProgram;

fn compile(src: &str) -> RillProgram<f32, 256> {
    compile_with_ffi::<f32>(src, &full_ffi::<f32>(), 44_100.0).unwrap()
}

/// Run one value tick and read its single scalar output.
fn scalar_output(prog: &mut RillProgram<f32, 256>) -> f64 {
    prog.run_value().unwrap();
    match prog.arena().get(prog.value_outputs()[0].unwrap()).unwrap() {
        Value::Float(v) => *v,
        other => panic!("expected a Float output, got {other:?}"),
    }
}

#[test]
fn actor_param_default_applies() {
    // `?gain=0.25` keeps its source default until the host writes the slot.
    let mut prog = compile("main = (?gain=0.25) * 2.0");
    assert_eq!(scalar_output(&mut prog), 0.5);
}

#[test]
fn actor_param_set_param_affects_output() {
    let mut prog = compile("main = (?gain=0.25) * 2.0");
    assert_eq!(scalar_output(&mut prog), 0.5);

    let idx = prog.param_index("gain").expect("?gain is a parameter slot");
    prog.set_param(idx, ParamValue::Float(4.0));
    assert_eq!(scalar_output(&mut prog), 8.0);
}

#[test]
fn actor_param_no_default_applies() {
    // A parameter with no source default starts at 0.0.
    let mut prog = compile("main = ?gain * 2.0");
    assert_eq!(scalar_output(&mut prog), 0.0);
}

#[test]
fn actor_param_persists_across_ticks() {
    let mut prog = compile("main = (?gain=1.0) * 2.0");
    let idx = prog.param_index("gain").unwrap();
    prog.set_param(idx, ParamValue::Float(3.0));

    assert_eq!(scalar_output(&mut prog), 6.0);
    // No new write: the slot keeps its value on the next tick.
    assert_eq!(scalar_output(&mut prog), 6.0);
}

#[test]
fn actor_param_reaches_a_block_builtin() {
    // A live parameter reaches an FFI block builtin: `sine`'s amplitude.
    let mut prog = compile("main = sine 440.0 ?level 0.0");

    let mut out = [0.0f32; 64];
    prog.run_value().unwrap();
    assert!(prog.copy_output_samples(0, &mut out));
    assert_eq!(out, [0.0; 64], "level default is 0.0 => silence");

    let idx = prog.param_index("level").unwrap();
    prog.set_param(idx, ParamValue::Float(0.5));
    prog.run_value().unwrap();
    assert!(prog.copy_output_samples(0, &mut out));
    assert!(
        out.iter().any(|v| v.abs() > 1e-3),
        "the live `level` must reach the generator"
    );
    assert!(out.iter().all(|v| v.abs() <= 0.5 + 1e-6));
}
