//! `atan2` as a language operation (mono buffer algebra), not an FFI builtin.

use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

/// A constant +1.0 buffer: a zero-frequency square wave at phase 0 stays in the
/// positive half-period, so it is the DC source for elementwise buffer ops.
const DC_ONE: &str = "square 0.0 1.0 0.0";

fn ffi() -> ForeignRegistry<f32> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    register::register_foreign_filters(&mut ffi);
    ffi
}

#[test]
fn atan2_over_buffers_is_elementwise() {
    // `atan2 a b : Buffer` is a curried value function over buffers
    // (elementwise). Feeding a known constant pair: (1.0, 1.0) → π/4.
    let src = format!("main = atan2 ({DC_ONE}) ({DC_ONE});");
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let ffi = ffi();
    let prog = rill_lang::test_eval::<f32, 256>(defs, expr, &ffi, 44100.0).unwrap();
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 64];
    assert!(prog.copy_buffer(r, &mut out));
    let expect = std::f32::consts::FRAC_PI_4;
    assert!(
        (out[0] - expect).abs() < 1e-4,
        "atan2(1,1) must be π/4, got {}",
        out[0]
    );
    assert!(out.iter().all(|v| (v - expect).abs() < 1e-4));
}

#[test]
fn atan2_in_value_position_lowers_to_buffer_bin() {
    // `atan2` over buffers lowers to `ValueBufferBin(Atan2)` — the curried
    // binary value function emits the buffer bin op when an operand is a
    // Buffer (here both, from the two DC generators).
    use rill_lang::ir::ValueInstr;
    use rill_lang::lower::lower_with_cafs;

    let src = format!("main = atan2 ({DC_ONE}) ({DC_ONE});");
    let toks = rill_lang::lexer::tokenize(&src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = lower_with_cafs(&typed, 44100.0, &typed.cafs).unwrap();
    assert!(
        ir.value_blocks
            .iter()
            .chain(ir.fragments.iter().flat_map(|f| &f.value_blocks))
            .flat_map(|b| &b.instrs)
            .any(|i| matches!(
                i,
                ValueInstr::ValueBufferBin {
                    op: rill_lang::ir::BinArith::Atan2,
                    ..
                }
            )),
        "atan2 must lower to ValueBufferBin(Atan2)"
    );
}
