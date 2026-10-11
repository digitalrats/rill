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
    // constant pair: (1.0, 1.0) → π/4. The 2-input bin op lowers through the
    // signal path; the channel counts are passed explicitly.
    let src = r#"
        main = _ , _ : atan2 1.0 1.0;
    "#;
    let mut prog = rill_lang::compile_with_ffi_channels::<f32>(src, &ffi(), 44100.0, 2, 1).unwrap();
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
    // `atan2` over two input channels lowers to `ValueBufferBin(Atan2)`. The
    // 2-input bin op has no 0-in value form, so it lowers through the signal
    // path with explicit channel counts.
    use rill_lang::ir::ValueInstr;
    use rill_lang::lower::lower_program_with_channels;

    let src = r#"
        main = _ , _ : atan2 1.0 1.0;
    "#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    let ir =
        lower_program_with_channels(&typed, 44100.0, &typed.cafs, typed.mode, 2, Some(1)).unwrap();
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
