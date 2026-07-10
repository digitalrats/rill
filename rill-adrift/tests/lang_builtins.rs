#![cfg(feature = "lang")]

use rill_adrift::lang_builtins::full_registry;
use rill_core::traits::Algorithm;
use rill_lang::compile_with;

fn run(src: &str, input: &[f32], sr: f32) -> Vec<f32> {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>(src, &reg, sr).unwrap();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(input), &mut out).unwrap();
    out
}

#[test]
fn onepole_sample_builtin_smooths() {
    let input: Vec<f32> = (0..64)
        .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
        .collect();
    let out = run("main = _ : onepole 200.0 0.7", &input, 48_000.0);
    let e: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(e < 0.9, "onepole did not attenuate (energy {e})");
}

#[test]
fn lowpass_block_matches_direct_biquad() {
    use rill_core_dsp::filters::{Biquad, FilterParams, FilterType};
    let input: Vec<f32> = (0..128).map(|i| (i as f32 * 0.3).sin()).collect();

    let via_lang = run("main = _ : lowpass 1000.0 0.7", &input, 48_000.0);

    let mut b = Biquad::<f32>::new(FilterParams {
        filter_type: FilterType::LowPass,
        cutoff: 1000.0,
        q: 0.7,
        gain_db: 0.0,
    });
    Algorithm::init(&mut b, 48_000.0);
    let mut direct = vec![0.0f32; input.len()];
    b.process(Some(&input), &mut direct).unwrap();

    for (i, (x, y)) in via_lang.iter().zip(direct.iter()).enumerate() {
        assert!((x - y).abs() < 1e-5, "sample {i}: lang {x} vs direct {y}");
    }
}

#[test]
fn sample_builtin_composes_in_feedback() {
    let reg = full_registry::<f32>();
    assert!(compile_with::<f32>("main = + ~ onepole 500.0 0.5", &reg, 48_000.0).is_ok());
}

#[test]
fn block_builtin_in_feedback_is_rejected() {
    let reg = full_registry::<f32>();
    let err = compile_with::<f32>("main = + ~ lowpass 500.0 0.7", &reg, 48_000.0);
    assert!(err.is_err());
}

#[cfg(feature = "analog")]
#[test]
fn analog_moog_smoke() {
    let reg = full_registry::<f32>();
    assert!(reg.get("analog_moog").is_some());
    assert!(compile_with::<f32>("main = _ : analog_moog 800.0 0.5", &reg, 48_000.0).is_ok());
}
