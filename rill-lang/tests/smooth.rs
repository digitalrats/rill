use rill_lang::compile_with_ffi;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;

// Task 2 (single-run value surface): `smooth_ramps_toward_target_across_ticks`
// is permanently removed. Its core assertion was CROSS-TICK convergence — the
// output ramping toward the target from a zero initial state across two
// `run_value()` ticks. The single-run model evaluates the program exactly once
// (`test_eval`), so no re-run surface exists to observe the ramp; the multi
// tick behavior cannot be re-expressed on the new surface.
//
// What survives is a compile check: `smooth (square ...) 5.0` must still lower
// through the FFI path at the reference 48 kHz rate. Cross-tick `smooth`
// convergence behavior itself is covered by the algorithm's own unit tests
// (rill-digital-effects).
#[test]
fn smooth_compiles_with_square_input() {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    let p = compile_with_ffi::<f32>("main = smooth (square 0.0 1.0 0.0) 5.0", &ffi, 48_000.0)
        .expect("smooth over a square generator must compile");
    let _ = p;
}
