use rill_core::traits::{Algorithm, ParamValue};
use rill_lang::builtin::Registry;
use rill_lang::compile_with;

#[test]
fn smooth_ramps_toward_target() {
    let mut p = compile_with::<f32>("main t = smooth t 5.0", &Registry::new(), 48_000.0).unwrap();
    p.set_param(p.param_index("t").unwrap(), ParamValue::Float(1.0));
    let mut out = [0.0f32; 64];
    p.process(Some(&[0.0f32; 64]), &mut out).unwrap();
    assert!(
        out[0] > 0.0 && out[0] < 1.0,
        "out[0]={} should be >0 <1",
        out[0]
    );
    assert!(
        out[63] > out[0],
        "out[63]={} should be > out[0]={}",
        out[63],
        out[0]
    );
    assert!(out[63] < 1.0, "out[63]={} should be <1", out[63]);
}

#[test]
fn smooth_hybrid_matches_reference() {
    let src = "main t = smooth t 20.0";
    let mut p_process = compile_with::<f32>(src, &Registry::new(), 48_000.0).unwrap();
    let mut p_ref = compile_with::<f32>(src, &Registry::new(), 48_000.0).unwrap();
    let ti = p_process.param_index("t").unwrap();
    p_process.set_param(ti, ParamValue::Float(1.0));
    p_ref.set_param(ti, ParamValue::Float(1.0));
    let mut out_process = [0.0f32; 64];
    let mut out_ref = [0.0f32; 64];
    let dummy = [0.0f32; 64];
    p_process.process(Some(&dummy), &mut out_process).unwrap();
    p_ref.process_reference(Some(&dummy), &mut out_ref).unwrap();
    let mut max_diff = 0.0f32;
    for i in 0..64 {
        let d = (out_process[i] - out_ref[i]).abs();
        if d > max_diff {
            max_diff = d;
        }
    }
    assert!(max_diff < 1e-4, "max_diff={} exceeds threshold", max_diff);
}
