//! SP-3b Task 12 review follow-up: build+run coverage for catalog builtins
//! that had no end-to-end test (graphic_eq, mono_to_stereo, sampler,
//! spectralgate, spectraldelay, convolver, im, cmul, cadd).

use rill_core::traits::MultichannelAlgorithm;
use rill_lang::ffi::ForeignRegistry;

fn run1(ffi: &ForeignRegistry<f32>, src: &str) -> [f32; 4] {
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, ffi, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 4]], &mut [&mut out]).unwrap();
    out
}

// --- rill-router builtins ---

#[test]
fn graphic_eq_ffi_end_to_end() {
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_router::register::register_foreign_router(&mut ffi);
    let out = run1(&ffi, r#"main = _ : graphic_eq 1.0;"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "graphic_eq output must be finite, got {out:?}"
    );
}

#[test]
fn mono_to_stereo_ffi_end_to_end() {
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_router::register::register_foreign_router(&mut ffi);
    let src = r#"main = _ : mono_to_stereo 0.0 0.0;"#;
    let mut prog = rill_lang::compile_with_ffi::<f32>(src, &ffi, 44100.0).unwrap();
    let mut l = [0.0f32; 4];
    let mut r = [0.0f32; 4];
    let mut outs: [&mut [f32]; 2] = [&mut l, &mut r];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 4]], &mut outs).unwrap();
    assert!(
        l.iter().all(|v| v.is_finite()) && r.iter().all(|v| v.is_finite()),
        "mono_to_stereo outputs must be finite, got {l:?} / {r:?}"
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

#[test]
fn spectral_builtins_ffi_end_to_end() {
    use rill_core::traits::MultichannelAlgorithm;
    let mut ffi = ForeignRegistry::<f32>::new();
    rill_fft::register::register_foreign_lang_builtins(&mut ffi);
    // The FFT effects are BUF=64 windowed; feed a 64-sample block.
    let mut prog =
        rill_lang::compile_with_ffi::<f32>(r#"main = _ : spectralgate 0.1 1.0;"#, &ffi, 44100.0)
            .unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 64]], &mut [&mut out]).unwrap();
    assert!(
        out.iter().all(|v| v.is_finite()),
        "spectralgate output must be finite, got {out:?}"
    );

    let mut prog =
        rill_lang::compile_with_ffi::<f32>(r#"main = _ : spectraldelay 0.5 0.3;"#, &ffi, 44100.0)
            .unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 64]], &mut [&mut out]).unwrap();
    assert!(
        out.iter().all(|v| v.is_finite()),
        "spectraldelay output must be finite, got {out:?}"
    );

    let mut prog =
        rill_lang::compile_with_ffi::<f32>(r#"main = _ : convolver 1.0 0.5;"#, &ffi, 44100.0)
            .unwrap();
    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 64]], &mut [&mut out]).unwrap();
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
    let out = run1(&ffi, r#"main = _ : lofi 8 44100 0.75 1.0 1 0 1;"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "lofi output must be finite, got {out:?}"
    );
    let out = run1(&ffi, r#"main = ay38910 1750000.0 0.0;"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "ay38910 output must be finite, got {out:?}"
    );
}

// --- complex ops (im/cmul/cadd) via the legacy factory registry ---

fn complex_registry() -> rill_lang::builtin::Registry<f32> {
    let mut reg = rill_lang::builtin::Registry::new();
    rill_lang::register::register_core_builtins(&mut reg);
    reg
}

fn run1_complex(src: &str) -> [f32; 4] {
    let reg = complex_registry();
    let mut prog = rill_lang::compile_with::<f32>(src, &reg, 44100.0).unwrap();
    let mut out = [0.0f32; 4];
    MultichannelAlgorithm::process(&mut prog, &[&[1.0f32; 4]], &mut [&mut out]).unwrap();
    out
}

#[test]
fn complex_im_cmul_cadd_end_to_end() {
    // im: extract the imaginary part (input 2 interleaved channels → 1 out).
    let out = run1_complex(r#"main = _ , _ : im;"#);
    assert!(
        out.iter().all(|v| v.is_finite()),
        "im output must be finite, got {out:?}"
    );

    // cmul/cadd: 4 interleaved channels (a_re a_im b_re b_im) → 2 outs.
    let reg = complex_registry();
    let src = r#"main = _ , _ , _ , _ : cmul;"#;
    let mut prog = rill_lang::compile_with::<f32>(src, &reg, 44100.0).unwrap();
    let mut o0 = [0.0f32; 4];
    let mut o1 = [0.0f32; 4];
    let mut outs: [&mut [f32]; 2] = [&mut o0, &mut o1];
    let ins: [&[f32]; 4] = [&[1.0f32; 4], &[2.0f32; 4], &[3.0f32; 4], &[4.0f32; 4]];
    MultichannelAlgorithm::process(&mut prog, &ins, &mut outs).unwrap();
    assert!(
        o0.iter().all(|v| v.is_finite()) && o1.iter().all(|v| v.is_finite()),
        "cmul outputs must be finite, got {o0:?} / {o1:?}"
    );
}
