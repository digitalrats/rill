//! Main λ-parameters are persistent runtime-stack cells: `SetParameter` writes
//! into the cell and the signal track materialises the cell's float value each
//! block. The cell must survive across ticks (a per-tick rebind would reset it
//! to `Void` and lose the set value).

use rill_core::traits::{MultichannelAlgorithm, ParamValue};
use rill_lang::compile;

#[test]
fn main_lambda_params_become_cells() {
    // main's λ-param `gain` is a runtime-stack cell, not intern_param.
    let mut prog = compile::<f32>("main gain = _ * gain").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(
        &mut prog,
        &[&[1.0f32, 2.0f32, 3.0f32, 4.0f32]],
        &mut [&mut out],
    )
    .unwrap();
    // gain defaults to 0 via the cell; output is 0.
    assert_eq!(out, [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn set_param_writes_into_the_cell() {
    // SetParameter must reach the SAME persistent cell and keep applying.
    let mut prog = compile::<f32>("main gain = _ * gain").unwrap();
    let idx = prog.param_index("gain").unwrap();
    prog.set_param(idx, ParamValue::Float(2.0));
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(
        &mut prog,
        &[&[1.0f32, 2.0f32, 3.0f32, 4.0f32]],
        &mut [&mut out],
    )
    .unwrap();
    assert_eq!(out, [2.0, 4.0, 6.0, 8.0]);
    // Second tick: the cell still holds 2.0 (persistence).
    MultichannelAlgorithm::process(
        &mut prog,
        &[&[1.0f32, 1.0f32, 1.0f32, 1.0f32]],
        &mut [&mut out],
    )
    .unwrap();
    assert_eq!(out, [2.0, 2.0, 2.0, 2.0]);
}

#[test]
fn main_param_reads_as_value_in_value_context() {
    // A main λ-param referenced from a value expression materialises the
    // persistent cell through the value track (`ValueReadMainCell`), so
    // `SetParameter` reaches value reads too. The match extracts a scalar
    // Float, keeping the output a single pinned arena slot (a compound
    // value output would pin its whole subtree — a separate, pre-existing
    // capacity limitation unrelated to main cells).
    let mut prog =
        compile::<f32>("data Shape = Circle Float; main g = match Circle g of { Circle r => r; }")
            .unwrap();
    prog.set_param(prog.param_index("g").unwrap(), ParamValue::Float(2.5));
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(
        prog.arena().get(prog.value_outputs()[0].unwrap()).unwrap(),
        &rill_lang::arena::Value::Float(2.5)
    );
    // The cell survives into the next tick (persistence).
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    assert_eq!(
        prog.arena().get(prog.value_outputs()[0].unwrap()).unwrap(),
        &rill_lang::arena::Value::Float(2.5)
    );
}
