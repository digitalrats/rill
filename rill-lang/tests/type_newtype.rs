//! End-to-end `type` / `newtype` tests: type synonyms substitute in field
//! types, and newtype wrappers construct and flow through the value track to
//! `RillProgram::value_outputs` as `Value::Newtype`.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::compile;

#[test]
fn newtype_construct_runs() {
    // `h = Hz 440.0` is a newtype constructor; the value output holds a
    // `Newtype` wrapping the inner `Float(440.0)`.
    let mut prog = compile::<f32>("newtype Hz = Float; h = Hz 440.0; main = h").unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    let inner = match val {
        rill_lang::arena::Value::Newtype(r) => *r,
        other => {
            panic!("expected a Newtype value, got {:?}", other);
        }
    };
    assert_eq!(
        prog.arena().get(inner).unwrap(),
        &rill_lang::arena::Value::Float(440.0)
    );
}

#[test]
fn type_synonym_substitutes() {
    // `type Angles = Float` substitutes into the record field type: `p.x`
    // projects a Float, proving the synonym resolved to Float at compile time.
    let mut prog = compile::<f32>(
        "type Angles = Float; data Point = { x: Angles, y: Angles }; p = Point { x: 1.0, y: 2.0 }; main = p.x",
    )
    .unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[], &mut [&mut out]).unwrap();
    let vo = prog.value_outputs();
    assert_eq!(vo.len(), 1);
    let v = vo[0].unwrap();
    let val = prog.arena().get(v).unwrap();
    assert_eq!(val, &rill_lang::arena::Value::Float(1.0));
}
