//! `?name` control parameters are persistent runtime cells: `SetParameter`
//! writes into the cell and both the value and signal tracks read it each
//! block. The cell must survive across ticks (a per-tick rebind would reset it
//! to `Void` and lose the set value).

use rill_core::traits::{Algorithm, ParamValue};
use rill_lang::compile;
use rill_lang::compile_with_ffi;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

fn value_output(prog: &rill_lang::program::RillProgram<f32, 256>) -> rill_lang::arena::Value {
    prog.arena()
        .get(prog.value_outputs()[0].unwrap())
        .unwrap()
        .clone()
}

#[test]
fn actor_param_defaults_to_zero() {
    // `?gain` is a runtime-stack cell, not intern_param; its default is 0.
    let mut prog = compile::<f32>("main = ?gain").unwrap();
    let mut out = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut out).unwrap();
    assert_eq!(value_output(&prog), rill_lang::arena::Value::Float(0.0));
}

#[test]
fn set_param_writes_into_the_cell() {
    // SetParameter must reach the SAME persistent cell and keep applying.
    let mut prog = compile::<f32>("main = ?gain * 1.0").unwrap();
    let idx = prog.param_index("gain").unwrap();
    prog.set_param(idx, ParamValue::Float(2.0));
    let mut out = [0.0f32; 1];
    Algorithm::process(&mut prog, None, &mut out).unwrap();
    assert_eq!(value_output(&prog), rill_lang::arena::Value::Float(2.0));
    // Second tick: the cell still holds 2.0 (persistence).
    Algorithm::process(&mut prog, None, &mut out).unwrap();
    assert_eq!(value_output(&prog), rill_lang::arena::Value::Float(2.0));
}

#[test]
fn param_reads_as_buffer_in_signal_context() {
    // A `?name` param read by a signal-track builtin reaches the persistent
    // cell, so `SetParameter` modulates the output across ticks.
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let mut prog =
        compile_with_ffi::<f32>("main = square 440.0 ?level 0.0", &ffi, 44100.0).unwrap();
    let idx = prog.param_index("level").unwrap();
    let mut out = [0.0f32; 4];
    Algorithm::process(&mut prog, None, &mut out).unwrap();
    assert_eq!(out, [0.0, 0.0, 0.0, 0.0], "level default is 0.0");
    prog.set_param(idx, ParamValue::Float(0.5));
    Algorithm::process(&mut prog, None, &mut out).unwrap();
    assert_eq!(out, [0.5, 0.5, 0.5, 0.5], "the cell holds 0.5");
}

#[test]
fn main_param_reads_as_value_in_value_context() {
    // A `?name` param referenced from a value expression materialises the
    // persistent cell through the value track (`ValueReadParam`), so
    // `SetParameter` reaches value reads too. The match extracts a scalar
    // Float, keeping the output a single pinned arena slot (a compound
    // value output would pin its whole subtree — a separate, pre-existing
    // capacity limitation unrelated to params).
    let mut prog =
        compile::<f32>(r"data Shape = Circle Float; main = case Circle ?g of { Circle r -> r; }")
            .unwrap();
    prog.set_param(prog.param_index("g").unwrap(), ParamValue::Float(2.5));
    let mut out = [0.0f32; 4];
    Algorithm::process(&mut prog, None, &mut out).unwrap();
    assert_eq!(value_output(&prog), rill_lang::arena::Value::Float(2.5));
    // The cell survives into the next tick (persistence).
    Algorithm::process(&mut prog, None, &mut out).unwrap();
    assert_eq!(value_output(&prog), rill_lang::arena::Value::Float(2.5));
}
