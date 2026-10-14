//! `atan2` as a language operation (mono buffer algebra), not an FFI builtin.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

fn ffi() -> ForeignRegistry<f32> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    register::register_foreign_filters(&mut ffi);
    ffi
}

#[test]
fn atan2_over_buffers_is_elementwise() {
    // `atan2 a b : Buffer` is a curried value function over buffers (elementwise)
    // and works on Float scalars too. Feeding a known constant pair: (1.0, 1.0)
    // → π/4. The value program maps the elementwise `atan2 x x` over the single
    // input channel (one constant 1.0 feed) — the unary-model equivalent of the
    // old two-input `_ , _ : atan2 1.0 1.0` signal wire.
    let src = r#"
        main = do { writeOutput (map (\x -> atan2 x x) (readInput)); return () };
    "#;
    let mut prog = rill_lang::compile_with_ffi_channels::<f32>(src, &ffi(), 44100.0, 1, 1).unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0; 64]], &mut [&mut out[..]]).unwrap();
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
    // binary value function emits the buffer bin op when an operand is a Buffer
    // (here both, from `map (\x -> atan2 x x) (readInput)`).
    use rill_lang::ir::ValueInstr;
    use rill_lang::lower::lower_with_cafs;

    let src = r#"
        main = do { writeOutput (map (\x -> atan2 x x) (readInput)); return () };
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
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
