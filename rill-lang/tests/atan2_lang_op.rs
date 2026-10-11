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
    // atan2 takes two buffers (y, x) → elementwise atan2. Feeding a known
    // constant pair: (1.0, 1.0) → π/4.
    let src = r#"
        main = _ , _ : atan2 1.0 1.0;
    "#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi(), 44100.0).unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0; 64], &[1.0; 64]], &mut [&mut out[..]])
        .unwrap();
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
    // IO-model value position: `atan2 i1 i2` as a writeOutput argument.
    use rill_lang::ir::ValueInstr;
    use rill_lang::lower::lower_with_cafs;

    let src = r#"
        main i1 i2 = do { writeOutput 0 (atan2 i1 i2); return () };
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir = lower_with_cafs(&typed, 44100.0, &typed.cafs).unwrap();
    assert!(
        ir.value_blocks
            .iter()
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
