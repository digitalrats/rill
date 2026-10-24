//! SP-3b Task 12 review follow-up: build+run coverage for catalog builtins
//! that had no end-to-end test (graphic_eq, sampler, spectralgate,
//! spectraldelay, convolver, lofi, ay38910).
pub mod common;

use common::*;
use rill_lang::ffi::ForeignRegistry;

fn run1(register: fn(&mut ForeignRegistry<f32>), src: &str) -> [f32; 4] {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let mut ffi = ForeignRegistry::<f32>::new();
    register(&mut ffi);
    let prog = eval_ffi(defs, expr, ffi).unwrap();
    let mut out = [0.0f32; 4];
    let r = prog.result().expect("whole-buffer result expected");
    assert!(prog.copy_buffer(r, &mut out));
    out
}

/// Register the EQ catalog and the DSP generators.
fn eq_registry(ffi: &mut ForeignRegistry<f32>) {
    rill_lang::register::register_foreign_eq(ffi);
    rill_lang::register::register_foreign_generators(ffi);
}

/// Register the sampler language builtins.
fn sampler_registry(ffi: &mut ForeignRegistry<f32>) {
    rill_sampler::register::register_foreign_lang_builtins(ffi);
}

/// Register the FFT catalog and the DSP generators.
fn spectral_registry(ffi: &mut ForeignRegistry<f32>) {
    rill_fft::register::register_foreign_lang_builtins(ffi);
    rill_lang::register::register_foreign_generators(ffi);
}

/// Register the lo-fi catalog and the DSP generators.
fn lofi_registry(ffi: &mut ForeignRegistry<f32>) {
    rill_lofi::register::register_foreign_lang_builtins(ffi);
    rill_lang::register::register_foreign_generators(ffi);
}

// --- EQ builtins (moved from rill-router to rill-core-dsp) ---

#[test]
fn graphic_eq_ffi_end_to_end() {
    let out = run1(
        eq_registry,
        r#"main = graphic_eq 1.0 (square 0.0 1.0 0.0);"#,
    );
    assert!(
        out.iter().all(|v| v.is_finite()),
        "graphic_eq output must be finite, got {out:?}"
    );
}

// --- rill-sampler builtin ---

#[test]
fn sampler_ffi_end_to_end() {
    // gate 1, rate 1, amp 0.5, cubic 0, source 0 — a fresh empty sample player.
    let out = run1(sampler_registry, r#"main = sampler 1.0 1.0 0.5 0.0 0.0;"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "sampler output must be finite, got {out:?}"
    );
}

// --- rill-fft builtins ---

/// Run a windowed `BUF = 64` spectral program: the FFT effects assert the
/// tick length equals their window size, so the program is built at
/// [`BUF`] = 64 rather than the default 256.
fn run_spectral(register: fn(&mut ForeignRegistry<f32>), src: &str) -> [f32; 64] {
    let (defs, expr) = src
        .rsplit_once("main = ")
        .expect("the source must bind `main` to the evaluated expression");
    let expr = expr.trim().trim_end_matches(';');
    let mut ffi = ForeignRegistry::<f32>::new();
    register(&mut ffi);
    let prog = eval_mode::<64>(defs, expr, ffi, rill_lang::mode::Mode::Strict).unwrap();
    let mut out = [0.0f32; 64];
    let r = prog.result().expect("whole-buffer result expected");
    assert!(prog.copy_buffer(r, &mut out));
    out
}

#[test]
fn spectral_builtins_ffi_end_to_end() {
    let out = run_spectral(
        spectral_registry,
        r#"main = spectralgate 0.1 1.0 (square 0.0 1.0 0.0);"#,
    );
    assert!(
        out.iter().all(|v| v.is_finite()),
        "spectralgate output must be finite, got {out:?}"
    );

    let out = run_spectral(
        spectral_registry,
        r#"main = spectraldelay 0.5 0.3 (square 0.0 1.0 0.0);"#,
    );
    assert!(
        out.iter().all(|v| v.is_finite()),
        "spectraldelay output must be finite, got {out:?}"
    );

    let out = run_spectral(
        spectral_registry,
        r#"main = convolver 1.0 0.5 (square 0.0 1.0 0.0);"#,
    );
    assert!(
        out.iter().all(|v| v.is_finite()),
        "convolver output must be finite, got {out:?}"
    );
}

// --- rill-lofi builtins ---

#[test]
fn lofi_builtins_ffi_end_to_end() {
    let out = run1(
        lofi_registry,
        r#"main = lofi 8 44100 0.75 1.0 1 0 1 (square 0.0 1.0 0.0);"#,
    );
    assert!(
        out.iter().all(|v| v.is_finite()),
        "lofi output must be finite, got {out:?}"
    );
    let out = run1(lofi_registry, r#"main = (ay38910 1750000.0 0.0);"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "ay38910 output must be finite, got {out:?}"
    );
}
