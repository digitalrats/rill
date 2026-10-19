//! Haskell 2010 strict data fields: `data P = { x: !Float, y: Float }` and
//! `data T = C !Int` force their arguments to WHNF at construction. A lazy
//! field stores the argument as a thunk, unforced; strictness never changes
//! the result of a terminating program.

use rill_lang::arena::Value;
use rill_lang::mode::Mode;

fn eval_main(src: &str, mode: Mode) -> (rill_lang::RillProgram<f32, 256>, rill_lang::ArenaRef) {
    let mut prog = rill_lang::compile_with_mode::<f32>(src, mode).expect("compile");
    let _ = prog.run_value();
    let v = prog.value_outputs()[0].expect("no value output");
    (prog, v)
}

#[test]
fn strict_record_field_divergence_is_bottom() {
    // `a = a` diverges; a strict `x` must force it AT CONSTRUCTION. In this
    // VM bottom is a first-class value: the force detects the divergence and
    // the field ends up WHNF (`Bottom`) — never an observable thunk.
    let (prog, v) = eval_main(
        "data P = { x: !Float, y: Float }; a = a; main = P { x: a, y: 1.0 }",
        Mode::Complete,
    );
    let fields = match prog.arena().get(v).unwrap() {
        Value::Record(fs) => fs.clone(),
        other => panic!("expected a Record, got {other:?}"),
    };
    match prog.arena().get(fields[0]).unwrap() {
        Value::Bottom => {}
        other => {
            panic!("expected the strict field to be Bottom (forced at construction), got {other:?}")
        }
    }
    match prog.arena().get(fields[1]).unwrap() {
        Value::Float(y) => assert_eq!(*y, 1.0),
        other => panic!("expected y = 1.0, got {other:?}"),
    }
}

#[test]
fn lazy_record_field_stores_divergence_unforced() {
    // The SAME program without `!` on `x` stores the SUSPENDED computation:
    // the record is WHNF, the field is a thunk. This pins that strictness is
    // per-field, not whole-ctor.
    let (prog, v) = eval_main(
        "data P = { x: Float, y: Float }; a = a; main = P { x: a, y: 1.0 }",
        Mode::Complete,
    );
    let fields = match prog.arena().get(v).unwrap() {
        Value::Record(fs) => fs.clone(),
        other => panic!("expected a Record, got {other:?}"),
    };
    match prog.arena().get(fields[0]).unwrap() {
        Value::Callable { cell: Some(_), .. } => {}
        other => panic!("expected an unforced thunk in the lazy field, got {other:?}"),
    }
}

#[test]
fn strict_sum_ctor_divergence_is_bottom() {
    let (prog, v) = eval_main(
        "data Opt = Has !Float | None; a = a; main = Has a",
        Mode::Complete,
    );
    let payload = match prog.arena().get(v).unwrap() {
        Value::Sum(0, ps) => ps.clone(),
        other => panic!("expected Sum(0, _), got {other:?}"),
    };
    match prog.arena().get(payload[0]).unwrap() {
        Value::Bottom => {}
        other => panic!(
            "expected the strict payload to be Bottom (forced at construction), got {other:?}"
        ),
    }
}

#[test]
fn lazy_sum_ctor_stores_payload_unforced() {
    let (prog, v) = eval_main(
        "data Opt = Has Float | None; a = a; main = Has a",
        Mode::Complete,
    );
    let payload = match prog.arena().get(v).unwrap() {
        Value::Sum(0, ps) => ps.clone(),
        other => panic!("expected Sum(0, _), got {other:?}"),
    };
    match prog.arena().get(payload[0]).unwrap() {
        Value::Callable { cell: Some(_), .. } => {}
        other => panic!("expected an unforced thunk in the lazy payload, got {other:?}"),
    }
}

fn compile_ir(src: &str, mode: Mode) -> rill_lang::Ir {
    let tokens = rill_lang::lexer::tokenize(src).expect("lex");
    let mut program = rill_lang::parser::parse(&tokens, src.as_bytes()).expect("parse");
    program.mode = mode;
    let typed = rill_lang::types::infer::infer_program(&program, mode).expect("infer");
    rill_lang::lower::lower_with_cafs(&typed, 44_100.0, &typed.cafs).expect("lower")
}

/// Count the `ValueForce` instructions in a lowered program, fragments included.
fn force_count(ir: &rill_lang::Ir) -> usize {
    ir.value_blocks
        .iter()
        .chain(ir.fragments.iter().flat_map(|f| &f.value_blocks))
        .flat_map(|b| &b.instrs)
        .filter(|i| matches!(i, rill_lang::ir::ValueInstr::ValueForce { .. }))
        .count()
}

#[test]
fn strict_record_field_emits_value_force() {
    let ir = compile_ir(
        "data P = { x: !Float, y: Float }; p = P { x: 1.0, y: 2.0 }; main = p.y",
        Mode::Complete,
    );
    assert_eq!(force_count(&ir), 1);
}

#[test]
fn lazy_record_field_emits_no_force() {
    let ir = compile_ir(
        "data P = { x: Float, y: Float }; p = P { x: 1.0, y: 2.0 }; main = p.y",
        Mode::Complete,
    );
    assert_eq!(force_count(&ir), 0);
}

#[test]
fn strict_sum_payload_emits_value_force() {
    assert_eq!(
        force_count(&compile_ir(
            "data T = C !Float; main = C 1.5",
            Mode::Complete
        )),
        1
    );
    assert_eq!(
        force_count(&compile_ir(
            "data T = C Float; main = C 1.5",
            Mode::Complete
        )),
        0
    );
}
