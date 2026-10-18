//! SP-3b Task 12 review follow-up: build+run coverage for catalog builtins
//! that had no end-to-end test (graphic_eq, sampler, spectralgate,
//! spectraldelay, convolver, lofi, ay38910).

use rill_lang::ffi::ForeignRegistry;
use rill_lang::test_eval;

fn run1(ffi: &ForeignRegistry<f32>, src: &str) -> [f32; 4] {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = test_eval::<f32, 256>(defs, expr, ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    let r = prog.result().expect("whole-buffer result expected");
    assert!(prog.copy_buffer(r, &mut out));
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
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let prog = test_eval::<f32, 64>(defs, expr, ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 64];
    let r = prog.result().expect("whole-buffer result expected");
    assert!(prog.copy_buffer(r, &mut out));
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
