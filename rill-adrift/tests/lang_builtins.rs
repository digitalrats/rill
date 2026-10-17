//! rill-lang builtins through rill-adrift's FFI registry, on the retained
//! value path: `main = <buffer expression>` produces one whole-buffer output
//! per tick, read back with `copy_output_samples`.

use rill_adrift::lang_builtins::full_ffi;
use rill_lang::compile_with_ffi;

const BUF: usize = 256;

/// Compile against rill-adrift's FFI registry and return one whole-buffer tick.
fn run(src: &str) -> Vec<f32> {
    let mut prog = compile_with_ffi::<f32>(src, &full_ffi::<f32>(), 48_000.0)
        .unwrap_or_else(|e| panic!("program must compile: {e}\nsource:\n{src}"));
    let mut out = vec![0.0f32; BUF];
    prog.run_value().unwrap();
    assert!(
        prog.copy_output_samples(0, &mut out),
        "program must produce a buffer output"
    );
    out
}

/// A constant-amplitude buffer source (`square` at zero frequency is +1.0).
const DC: &str = "square 0.0 1.0 0.0";

#[test]
fn onepole_builtin_smooths() {
    let out = run("main = onepole 200.0 0.7 (square 440.0 1.0 0.0)");
    let e: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(e < 0.9, "onepole did not attenuate (energy {e})");
}

#[test]
fn integrator_short_form_runs_sum() {
    let out = run(&format!("main = integrator ({DC})"));
    assert_eq!(&out[..4], &[1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn leaky_integrator_short_form_smooths() {
    let out = run(&format!("main = leaky_integrator 0.5 ({DC})"));
    assert!((out[0] - 1.0).abs() < 1e-6);
    assert!((out[1] - 1.5).abs() < 1e-6);
    assert!((out[2] - 1.75).abs() < 1e-6);
    assert!((out[3] - 1.875).abs() < 1e-6);
}

#[test]
fn lowpass_block_matches_direct_biquad() {
    use rill_core::traits::Algorithm;
    use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};

    let via_lang = run(&format!("main = lowpass 1000.0 0.7 ({DC})"));

    let input = vec![1.0f32; BUF];
    let mut b = Biquad::<f32>::new(FilterParams {
        filter_type: FilterType::LowPass,
        cutoff: 1000.0,
        q: 0.7,
        gain_db: 0.0,
    });
    Algorithm::init(&mut b, 48_000.0);
    let mut direct = vec![0.0f32; BUF];
    b.process(Some(&input), &mut direct).unwrap();

    for (i, (x, y)) in via_lang.iter().zip(direct.iter()).enumerate() {
        assert!((x - y).abs() < 1e-5, "sample {i}: lang {x} vs direct {y}");
    }
}

#[cfg(feature = "analog")]
#[test]
fn analog_moog_smoke() {
    // `analog_moog` moved out of rill-core-model into rill-lang's `model`
    // feature (SP-3b Task 9): the legacy registry no longer carries it, the FFI
    // factory does.
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_model;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_model(&mut ffi);
    assert!(compile_with_ffi::<f32>(
        &format!("main = analog_moog 800.0 0.5 ({DC})"),
        &ffi,
        48_000.0,
    )
    .is_ok());
}
