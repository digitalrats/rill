//! End-to-end data-program tests: `data` declarations flow through the whole
//! pipeline (infer → reduce → lower → executor) and the program's value
//! outputs are exposed via `RillProgram::value_outputs`.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn record_flow_through_process() {
    // `p` is a value binding (CAF) whose body is a record constructor; `main`
    // projects a field. The program must compile and run a tick, and the value
    // output must hold Float(2.0) — the projected `p.x`.
    let mut prog = compile::<f32>(
        "data Point = { x: Float, y: Float }; p = Point { x: 2.0, y: 3.0 }; main = p.x",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(2.0));
}

#[test]
fn value_output_survives_across_ticks() {
    // The value output is an independent counted owner of its arena ref: it
    // must stay readable after `process` returns, and the fixed arena must not
    // exhaust on a second tick (the outputs keep one slot alive across ticks,
    // so the capacity heuristic must account for them).
    let mut prog = compile::<f32>(
        "data Point = { x: Float, y: Float }; p = Point { x: 2.0, y: 3.0 }; main = p.x",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..2 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert_eq!(
            prog.arena().get(v).unwrap(),
            &rill_lang::arena::Value::Float(2.0)
        );
    }
}

#[test]
fn compound_value_output_survives_ticks() {
    // A compound (Record) value output pins its WHOLE subtree across ticks: the
    // output keeps the record at rc >= 1 forever, so its field slots never
    // free. The arena capacity must account for the subtree size, otherwise
    // tick 2 panics with "value arena capacity exhausted at build time".
    let mut prog =
        compile::<f32>("data Point = { x: Float, y: Float }; main = Point { x: 2.0, y: 3.0 }")
            .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert!(
            matches!(
                prog.arena().get(v).unwrap(),
                &rill_lang::arena::Value::Record(_)
            ),
            "output must hold the Point record on every tick"
        );
    }
}

#[test]
fn nested_record_output_survives_ticks() {
    // Nested records pin the full subtree transitively: the outer record holds
    // the inner record's slot, which in turn holds its fields' slots.
    let mut prog = compile::<f32>(
        "data Point = { x: Float, y: Float }; data Line = { a: Point, b: Point }; main = Line { a: Point { x: 1.0, y: 2.0 }, b: Point { x: 3.0, y: 4.0 } }",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert!(
            matches!(
                prog.arena().get(v).unwrap(),
                &rill_lang::arena::Value::Record(_)
            ),
            "output must hold the Line record on every tick"
        );
    }
}

#[test]
fn sum_value_output_survives_ticks() {
    // A Sum output pins its payload across ticks (1 + max-payload subtree).
    let mut prog =
        compile::<f32>("data Shape = Circle Float | Rect Float Float; main = Rect 1.0 2.0")
            .unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert!(
            matches!(
                prog.arena().get(v).unwrap(),
                &rill_lang::arena::Value::Sum(..)
            ),
            "output must hold the Rect sum on every tick"
        );
    }
}

#[test]
fn newtype_value_output_survives_ticks() {
    // A Newtype output pins its inner value across ticks (1 + inner subtree).
    let mut prog = compile::<f32>("newtype Hz = Float; main = Hz 440.0").unwrap();
    let mut out = [0.0f32; 4];
    for _ in 0..3 {
        MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
        let v = prog.value_outputs()[0].unwrap();
        assert!(
            matches!(
                prog.arena().get(v).unwrap(),
                &rill_lang::arena::Value::Newtype(_)
            ),
            "output must hold the Hz newtype on every tick"
        );
    }
}
