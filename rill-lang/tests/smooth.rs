use rill_core::traits::{Algorithm, ParamValue};
use rill_lang::builtin::Registry;
use rill_lang::compile_with;

#[test]
fn smooth_ramps_toward_target_across_ticks() {
    let mut p = compile_with::<f32>("main t = smooth t 5.0", &Registry::new(), 48_000.0).unwrap();
    p.set_param(p.param_index("t").unwrap(), ParamValue::Float(1.0));
    let mut out = [0.0f32; 64];
    p.process(Some(&[0.0f32; 64]), &mut out).unwrap();
    let first = out[0];
    assert!(
        first > 0.0 && first < 1.0,
        "out[0]={} should be >0 <1",
        first
    );

    p.process(Some(&[0.0f32; 64]), &mut out).unwrap();
    assert!(out[0] > first, "out[0]={} should ramp past {first}", out[0]);
    assert!(out[0] < 1.0, "out[0]={} should be <1", out[0]);
}
