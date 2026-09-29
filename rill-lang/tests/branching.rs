//! `if` as a pure expression: static selection, result binding, and runtime
//! per-tick switching driven by SetParameter and value-state feedback.

use rill_core::traits::{MultichannelAlgorithm, ParamValue};
use rill_lang::{compile, RillProgram};

fn float_output(prog: &mut RillProgram<f32, 256>, out: &mut [f32; 4]) -> f64 {
    MultichannelAlgorithm::process(prog, &[], &mut [out]).unwrap();
    let v = prog.value_outputs()[0].unwrap();
    match prog.arena().get(v).unwrap() {
        rill_lang::arena::Value::Float(f) => *f,
        other => panic!("unexpected output {other:?}"),
    }
}

#[test]
fn if_static_branches() {
    let mut prog = compile::<f32>("main = if true then 1.0 else 2.0;").unwrap();
    let mut out = [0.0f32; 4];
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
    let mut prog = compile::<f32>("main = if false then 1.0 else 2.0;").unwrap();
    assert_eq!(float_output(&mut prog, &mut out), 2.0);
}

#[test]
fn if_binds_result_and_nests() {
    let mut prog = compile::<f32>("x = if false then 1.0 else 2.0; main = 0.5 * x;").unwrap();
    let mut out = [0.0f32; 4];
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
}

#[test]
fn if_switches_on_setparameter_between_ticks() {
    // SetParameter → main cell → per-tick re-evaluation → branch switches.
    let mut prog = compile::<f32>("main g = if g > 0.5 then 1.0 else 2.0;").unwrap();
    let mut out = [0.0f32; 4];
    // default g = 0 → else branch
    assert_eq!(float_output(&mut prog, &mut out), 2.0);
    let idx = prog.param_index("g").unwrap();
    prog.set_param(idx, ParamValue::Float(1.0));
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
    prog.set_param(idx, ParamValue::Float(0.0));
    assert_eq!(float_output(&mut prog, &mut out), 2.0);
}

// NOTE: a value-track feedback accumulator (`acc = ~ (acc + 1.0)`) is not
// testable in v1 — `~`/`@` over values is not wired in lowering (lower.rs:
// "Value-state slots are not emitted by lowering in v1"), so no value-track
// program can switch across ticks except via a SetParameter-driven main cell,
// which `if_switches_on_setparameter_between_ticks` already covers.

#[test]
fn if_and_runtime_match_combined() {
    // A SetParameter-driven Bool selects a sum at runtime; the outer match
    // dispatches on the runtime constructor.
    let mut prog = compile::<f32>(
        "main g = match (if g > 0.5 then Just 1.0 else Nothing) of { Just x => x; Nothing => 0.0; };",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    assert_eq!(float_output(&mut prog, &mut out), 0.0);
    prog.set_param(prog.param_index("g").unwrap(), ParamValue::Float(1.0));
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
}

#[test]
fn if_nested_in_expression_at_runtime() {
    // A RUNTIME if nested inside an arithmetic expression: the surrounding
    // expression must continue in the if's join block. This guards the
    // `cur_value_block = join` wiring (a regression there would silently run
    // the continuation on only one path).
    let mut prog = compile::<f32>("main g = 0.5 * (if g > 0.5 then 2.0 else 4.0);").unwrap();
    let mut out = [0.0f32; 4];
    // default g = 0 → else → 0.5 * 4.0 = 2.0
    assert_eq!(float_output(&mut prog, &mut out), 2.0);
    prog.set_param(prog.param_index("g").unwrap(), ParamValue::Float(1.0));
    // then → 0.5 * 2.0 = 1.0
    assert_eq!(float_output(&mut prog, &mut out), 1.0);
}
