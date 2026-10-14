//! Spec smoke: a program written in the full Haskell 2010 surface syntax
//! (backslash lambdas, `--` line comments, layout/offside `do`, curried
//! application) compiles and runs through the FFI channel path.

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

#[test]
fn haskell2010_syntax_end_to_end() {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_filters(&mut ffi);
    // The whole program is written in the new surface: `--` comments, `\x ->`
    // lambdas, top-level defs separated by lines (no braces/semicolons), a
    // partial application of a foreign fn (`onepole 200.0 0.7`), and a layout
    // `do` block whose statements live at the same indentation column.
    let src = r#"
        -- gain and filter composition
        gain = \x -> x * 0.5
        filt = \x -> onepole 200.0 0.7 x
        main = do
            writeOutput (map filt (map gain (readInput)))
            return ()
    "#;
    let mut prog = rill_lang::compile_with_ffi_channels::<f32>(src, &ffi, 44100.0, 1, 1)
        .expect("the Haskell 2010 program must compile");
    let mut out = [0.0f32; 16];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 16]], &mut [&mut out])
        .expect("the compiled program must run");
    assert!(
        out.iter().all(|v| v.is_finite()),
        "output must be finite, got {out:?}"
    );
}
