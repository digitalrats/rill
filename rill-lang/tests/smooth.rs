use rill_core::traits::Algorithm;
use rill_lang::compile_with_ffi;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

#[test]
fn smooth_ramps_toward_target_across_ticks() {
    // The target is a constant +1.0 buffer (a zero-frequency square wave in its
    // positive half-period); `smooth` converges toward it across ticks from a
    // zero initial state.
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let mut p =
        compile_with_ffi::<f32>("main = smooth (square 0.0 1.0 0.0) 5.0", &ffi, 48_000.0).unwrap();
    let mut out = [0.0f32; 64];
    p.process(None, &mut out).unwrap();
    let first = out[0];
    assert!(
        first > 0.0 && first < 1.0,
        "out[0]={} should be >0 <1",
        first
    );

    p.process(None, &mut out).unwrap();
    assert!(out[0] > first, "out[0]={} should ramp past {first}", out[0]);
    assert!(out[0] < 1.0, "out[0]={} should be <1", out[0]);
}
