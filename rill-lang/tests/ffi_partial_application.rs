//! FFI partial application (currying): `onepole 200.0 0.7 : Buffer -> Buffer`
//! is a first-class `Func` value; scalars fold at build, the Buffer stays open.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

fn ffi() -> ForeignRegistry<f32> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_filters(&mut ffi);
    register::register_foreign_generators(&mut ffi);
    ffi
}

#[test]
fn partial_application_is_first_class() {
    // fp = onepole 200.0 0.7 : Buffer -> Buffer; mapping it over the program's
    // input list compiles and runs (a signal-arrow typing would have arity
    // 1->1 and fail the value-track map application).
    let src = r#"fp = onepole 200.0 0.7; main = fp (square 0.0 1.0 0.0);"#;
    let mut prog = rill_lang::compile_with_ffi_channels::<f32>(src, &ffi(), 44100.0, 1, 1).unwrap();
    let ones = [1.0f32; 64];
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[&ones], &mut [&mut out[..]]).unwrap();
    assert!(out.iter().all(|v| v.is_finite()));
}

#[test]
fn partial_application_matches_direct_call() {
    // fp i1 === onepole 200.0 0.7 i1.
    let direct = r#"main = onepole 200.0 0.7 (square 0.0 1.0 0.0);"#;
    let via_fp = r#"fp = onepole 200.0 0.7; main = fp (square 0.0 1.0 0.0);"#;
    let run = |src: &str| -> Vec<f32> {
        let mut prog =
            rill_lang::compile_with_ffi_channels::<f32>(src, &ffi(), 44100.0, 1, 1).unwrap();
        let mut out = [0.0f32; 64];
        MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 64]], &mut [&mut out[..]]).unwrap();
        out.to_vec()
    };
    let a = run(direct);
    let b = run(via_fp);
    assert_eq!(a, b, "fp application must equal the direct call");
    assert!(
        a[0] > 0.0,
        "onepole must pass the DC step, got out[0]={}",
        a[0]
    );
}

#[test]
fn partial_application_keeps_scalars_folded() {
    use rill_lang::lower::lower_with_cafs;
    let src = r#"fp = onepole 200.0 0.7; main = fp (square 0.0 1.0 0.0);"#;
    let toks = rill_lang::lexer::tokenize(src).unwrap();
    let prog = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let mut typed = rill_lang::types::infer::infer_program(&prog, prog.mode).unwrap();
    // The compile pipeline reduces before lowering: `fp` is inlined at its use
    // site, so `main = fp (…)` is an ordinary buffer application.
    typed.program = rill_lang::reduce::reduce(&typed.program);
    let ir = lower_with_cafs(&typed, 44100.0, &typed.cafs).unwrap();
    let bi = ir
        .builtins
        .iter()
        .find(|b| b.name == "onepole")
        .expect("onepole builtin");
    assert_eq!(bi.params, vec![200.0, 0.7], "scalars fold into the builtin");
    // The FFI arrow is a Callable fragment taking one Buffer.
    let one = ir
        .fragments
        .iter()
        .find(|f| f.sig.value_ins == 1)
        .expect("arrow fragment");
    assert_eq!(one.sig.value_outs, 1);
}
