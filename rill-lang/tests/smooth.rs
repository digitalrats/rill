use rill_core::traits::Algorithm;
use rill_lang::compile_with_ffi_channels;
use rill_lang::ffi::ForeignRegistry;

#[test]
fn smooth_ramps_toward_target_across_ticks() {
    // IO model: the target is the program's BUFFER input (`i1`), not a
    // SetParameter control.
    let mut p = compile_with_ffi_channels::<f32>(
        "main i1 = do { writeOutput [smooth i1 5.0]; return () }",
        &ForeignRegistry::new(),
        48_000.0,
        1,
        1,
    )
    .unwrap();
    let mut out = [0.0f32; 64];
    p.process(Some(&[1.0f32; 64]), &mut out).unwrap();
    let first = out[0];
    assert!(
        first > 0.0 && first < 1.0,
        "out[0]={} should be >0 <1",
        first
    );

    p.process(Some(&[1.0f32; 64]), &mut out).unwrap();
    assert!(out[0] > first, "out[0]={} should ramp past {first}", out[0]);
    assert!(out[0] < 1.0, "out[0]={} should be <1", out[0]);
}
