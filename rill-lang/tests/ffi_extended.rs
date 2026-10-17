//! SP-3b Task 12 review follow-up: build+run coverage for catalog builtins
//! that had no end-to-end test (graphic_eq, sampler, spectralgate,
//! spectraldelay, convolver, lofi, ay38910).

use rill_lang::ffi::ForeignRegistry;

fn run1(ffi: &ForeignRegistry<f32>, src: &str) -> [f32; 4] {
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    prog.run_value().unwrap();
    assert!(prog.copy_output_samples(0, &mut out));
    out
}

// --- EQ builtins (moved from rill-router to rill-core-dsp) ---

#[test]
fn graphic_eq_ffi_end_to_end() {
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_lang::register::register_foreign_eq(&mut ffi);
    rill_lang::register::register_foreign_generators(&mut ffi);
    let out = run1(&ffi, r#"main = graphic_eq 1.0 (square 0.0 1.0 0.0);"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "graphic_eq output must be finite, got {out:?}"
    );
}

// --- rill-sampler builtin ---

#[test]
fn sampler_ffi_end_to_end() {
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_sampler::register::register_foreign_lang_builtins(&mut ffi);
    // gate 1, rate 1, amp 0.5, cubic 0, source 0 — a fresh empty sample player.
    let out = run1(&ffi, r#"main = sampler 1.0 1.0 0.5 0.0 0.0;"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "sampler output must be finite, got {out:?}"
    );
}

// --- rill-fft builtins ---

/// Run a windowed `BUF = 64` spectral program: the FFT effects assert the
/// tick length equals their window size, so the program is built at
/// [`BUF`] = 64 rather than the default 256.
fn run_spectral(ffi: &ForeignRegistry<f32>, src: &str) -> [f32; 64] {
    let src = src.to_string();
    let toks = rill_lang::lexer::tokenize(&src).unwrap();
    let ast = rill_lang::parser::parse(&toks, src.as_bytes()).unwrap();
    let typed = rill_lang::types::infer::infer_program(&ast, ast.mode).unwrap();
    let ir = rill_lang::lower::lower_with_cafs(&typed, 44100.0, &typed.cafs).unwrap();
    let mut prog = rill_lang::RillProgram::<f32, 64>::new_with(
        ir,
        &rill_lang::Registry::<f32>::new(),
        44100.0,
        Some(ffi),
    )
    .unwrap();
    let mut out = [0.0f32; 64];
    prog.run_value().unwrap();
    assert!(prog.copy_output_samples(0, &mut out));
    out
}

#[test]
fn spectral_builtins_ffi_end_to_end() {
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_fft::register::register_foreign_lang_builtins(&mut ffi);
    rill_lang::register::register_foreign_generators(&mut ffi);

    let out = run_spectral(&ffi, r#"main = spectralgate 0.1 1.0 (square 0.0 1.0 0.0);"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "spectralgate output must be finite, got {out:?}"
    );

    let out = run_spectral(
        &ffi,
        r#"main = spectraldelay 0.5 0.3 (square 0.0 1.0 0.0);"#,
    );
    assert!(
        out.iter().all(|v| v.is_finite()),
        "spectraldelay output must be finite, got {out:?}"
    );

    let out = run_spectral(&ffi, r#"main = convolver 1.0 0.5 (square 0.0 1.0 0.0);"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "convolver output must be finite, got {out:?}"
    );
}

// --- rill-lofi builtins ---

#[test]
fn lofi_builtins_ffi_end_to_end() {
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_lofi::register::register_foreign_lang_builtins(&mut ffi);
    rill_lang::register::register_foreign_generators(&mut ffi);
    let out = run1(
        &ffi,
        r#"main = lofi 8 44100 0.75 1.0 1 0 1 (square 0.0 1.0 0.0);"#,
    );
    assert!(
        out.iter().all(|v| v.is_finite()),
        "lofi output must be finite, got {out:?}"
    );
    let out = run1(&ffi, r#"main = (ay38910 1750000.0 0.0);"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "ay38910 output must be finite, got {out:?}"
    );
}
