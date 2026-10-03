use rill_adrift::lang_builtins::full_registry;
use rill_core::traits::Algorithm;
use rill_lang::compile_graph;
use rill_lang::compile_with;

fn run(src: &str, input: &[f32], sr: f32) -> Vec<f32> {
    let reg = full_registry::<f32>();
    let mut prog = compile_with::<f32>(src, &reg, sr).unwrap();
    let mut out = vec![0.0f32; input.len()];
    prog.process(Some(input), &mut out).unwrap();
    out
}

#[test]
fn onepole_builtin_smooths() {
    let input: Vec<f32> = (0..64)
        .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
        .collect();
    let out = run("main = _ : onepole 200.0 0.7", &input, 48_000.0);
    let e: f32 = out.iter().map(|x| x * x).sum::<f32>() / out.len() as f32;
    assert!(e < 0.9, "onepole did not attenuate (energy {e})");
}

#[test]
fn integrator_short_form_runs_sum() {
    // `+ ~ _` desugars to the `integrator` built-in.
    let out = run("main = + ~ _", &[1.0, 1.0, 1.0, 1.0], 48_000.0);
    assert_eq!(out, vec![1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn leaky_integrator_short_form_smooths() {
    // `+ ~ (_ * 0.5)` desugars to `leaky_integrator 0.5`.
    let out = run("main = + ~ (_ * 0.5)", &[1.0, 1.0, 1.0, 1.0], 48_000.0);
    assert!((out[0] - 1.0).abs() < 1e-6);
    assert!((out[1] - 1.5).abs() < 1e-6);
    assert!((out[2] - 1.75).abs() < 1e-6);
    assert!((out[3] - 1.875).abs() < 1e-6);
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
fn builtin_composes_in_feedback() {
    let reg = full_registry::<f32>();
    assert!(compile_with::<f32>("main = + ~ onepole 500.0 0.5", &reg, 48_000.0).is_ok());
}

#[test]
fn block_builtin_in_feedback_compiles() {
    let reg = full_registry::<f32>();
    assert!(compile_with::<f32>("main = + ~ lowpass 500.0 0.7", &reg, 48_000.0).is_ok());
}

#[cfg(feature = "analog")]
#[test]
fn analog_moog_smoke() {
    // `analog_moog` moved out of rill-core-model into rill-lang's `model`
    // feature (SP-3b Task 9): the legacy `full_registry` no longer carries it,
    // the FFI factory does.
    use rill_lang::compile_with_ffi;
    use rill_lang::ffi::ForeignRegistry;
    use rill_lang::register::register_foreign_model;

    let mut ffi = ForeignRegistry::<f32>::new();
    register_foreign_model(&mut ffi);
    assert!(compile_with_ffi::<f32>("main = _ : analog_moog 800.0 0.5", &ffi, 48_000.0).is_ok());
}

#[test]
fn dry_wet_blends() {
    use rill_core::traits::MultichannelAlgorithm;
    let reg = full_registry::<f32>();
    let mut engine =
        compile_graph::<f32, 256>("main = (_, _) :> dry_wet { mix: 1.0 }", &reg, 48_000.0).unwrap();
    let dry = [1.0f32; 4];
    let wet = [3.0f32; 4];
    let mut l = [0.0f32; 4];
    let mut r = [0.0f32; 4];
    let inputs: [&[f32]; 2] = [&dry, &wet];
    let mut outputs: [&mut [f32]; 2] = [&mut l, &mut r];
    MultichannelAlgorithm::process(&mut engine, &inputs, &mut outputs).unwrap();
    assert_eq!(l, [3.0f32; 4], "mix=1.0 should pass wet through");
}

#[test]
fn mixer_sums_stereo() {
    use rill_core::traits::MultichannelAlgorithm;
    let reg = full_registry::<f32>();
    let mut engine =
        compile_graph::<f32, 256>("main = (_, _) :> mixer { master_vol: 1.0 }", &reg, 48_000.0)
            .unwrap();
    let ch0 = [1.0f32; 4];
    let ch1 = [2.0f32; 4];
    let mut l = [0.0f32; 4];
    let mut r = [0.0f32; 4];
    let inputs: [&[f32]; 2] = [&ch0, &ch1];
    let mut outputs: [&mut [f32]; 2] = [&mut l, &mut r];
    MultichannelAlgorithm::process(&mut engine, &inputs, &mut outputs).unwrap();
    assert!((l[0] - 2.4).abs() < 1e-5, "l[0]={} expected ~2.4", l[0]);
}
