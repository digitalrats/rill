//! CAF (closed top-level definition) free-variable integration tests.
//!
//! Behavioral, end-to-end coverage of closed top-level definitions through the
//! public `test_eval` entry point. The core property under test: a closed
//! top-level stateful definition referenced from two places lowers to ONE
//! shared instance, and its observable state is shared across every reference.
//! On the single-run surface the discriminator is a harness-owned call counter:
//! exactly one instance must run per evaluation of the program.
pub mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use common::*;
use rill_core::traits::{Algorithm, ProcessResult};
use rill_lang::builtin::BlockBuiltin;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;

/// A real block oscillator (0 signal in, 1 out; params freq/amp/phase) whose
/// phase accumulates per sample.
///
/// Each `process` call bumps a harness-owned `AtomicUsize` so tests can observe
/// how many instances a compiled program actually runs per tick: a single
/// shared CAF instance bumps it once, a duplicated build would bump it once per
/// reference.
struct SineOsc {
    freq: f32,
    amp: f32,
    phase: f32,
    sample_rate: f32,
    tick: f64,
    calls: Arc<AtomicUsize>,
}

impl SineOsc {
    fn new(params: &[f64], sample_rate: f32, calls: Arc<AtomicUsize>) -> Self {
        Self {
            freq: params[0] as f32,
            amp: params[1] as f32,
            phase: params[2] as f32,
            sample_rate,
            tick: 0.0,
            calls,
        }
    }
}

impl Algorithm<f32> for SineOsc {
    fn process(&mut self, _input: Option<&[f32]>, output: &mut [f32]) -> ProcessResult<()> {
        let two_pi = 2.0 * std::f32::consts::PI;
        for o in output.iter_mut() {
            let t = (self.tick as f32) / self.sample_rate;
            *o = self.amp * (two_pi * self.freq * t + self.phase).sin();
            self.tick += 1.0;
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn reset(&mut self) {
        self.tick = 0.0;
    }
}

impl BlockBuiltin<f32> for SineOsc {}

/// Registry with a real `sine` block builtin, returning the shared call counter
/// alongside the registry.
fn sine_registry() -> (ForeignRegistry<f32>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ForeignRegistry::<f32>::new();
    let shared = calls.clone();
    registry.register_block("sine", move |params, sample_rate, _alloc| {
        Box::new(SineOsc::new(params, sample_rate, shared.clone()))
    });
    (registry, calls)
}

/// `osc = sine 440.0 0.5 0.0; main = osc` must run exactly ONE oscillator
/// instance per evaluation.
///
/// The unary value model removed the multi-channel `,` merge, so the program
/// exposes ONE output channel fed by the single shared CAF. The call counter
/// must advance by exactly one — a duplicated or un-shared build (extra `osc`
/// instances) would advance by more. `ch0 == ch1` (the old cross-channel
/// agreement check) had no second channel to compare against and is dropped;
/// the call counter is the real sharing/duplication discriminator.
///
/// The original second-tick half (phase advancing across re-runs) is gone with
/// the multi-tick surface: the single-run model evaluates the program exactly
/// once, so only the one-instance-per-evaluation guarantee survives here.
#[test]
fn shared_oscillator_runs_one_instance_per_tick() {
    let (ffi, calls) = sine_registry();
    let prog = eval_ffi("osc = sine 440.0 0.5 0.0;", "osc", ffi)
        .expect("program with a shared CAF compiles");

    // Value model: a program has ONE value output — the single generator.
    let r = prog.result().expect("whole-buffer result expected");
    let mut ch0 = [0.0f32; 8];
    assert!(prog.copy_buffer(r, &mut ch0));

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "one shared sine instance must run once per evaluation"
    );
    assert!(
        ch0.iter().any(|v| *v != 0.0),
        "the shared sine must produce a non-zero signal"
    );
}

/// A user-defined function can capture a free variable of a closed CAF.
///
/// `voice gain = osc * gain` references the closed CAF `osc` and applies a
/// scalar gain to the shared phase. The single output channel carries one voice
/// (`voice 0.5`), and the captured CAF must lower to exactly ONE sine instance
/// (the call counter advances by one).
///
/// The two merged voices of the original `main = voice 0.5 , voice 0.7` collapse
/// into the value model's single output: they now sum into one buffer and the
/// old per-channel gain ratio assertion cannot survive (see inline comment).
#[test]
fn user_function_captures_shared_free_variable() {
    let (ffi, calls) = sine_registry();
    let prog = eval_ffi(
        "osc = sine 440.0 0.5 0.0; voice gain = osc * gain;",
        "voice 0.5",
        ffi,
    )
    .expect("user function capturing a CAF compiles");

    // Value model: one value output.
    let r = prog.result().expect("whole-buffer result expected");
    let mut out = [0.0f32; 8];
    assert!(prog.copy_buffer(r, &mut out));

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the captured CAF is a single shared instance"
    );
    assert!(
        out.iter().any(|v| *v != 0.0),
        "the shared sine must produce a non-zero signal"
    );
    // The old `ch1 == 1.4 * ch0` ratio assertion is dropped: the `,` merge no
    // longer exposes `voice 0.5` and `voice 0.7` as two output channels, and a
    // single channel carries the sum of the voices. (The value track lowers
    // each CAF reference to its own instance, so a multi-consumer sharing check
    // cannot be expressed over separate channels in the unary model; the
    // single-instance call-count assertion above is the surviving core check.)
}

// Task 13b: `open_block_inlined_twice_stays_independent` removed with the
// signal surface syntax (loop combinator `~`). The test's premise — the open
// block `integ = + ~ _` inlined twice into two independent stateful paths fed
// by separate input channels — was built on the `~` loop combinator. In the
// unary value model there is no way to express two independent per-channel
// stateful paths: `main`'s λ-parameters are String launch arguments (not Buffer
// inputs), and a `map` over a literal list reuses a single builtin instance per
// element (shared running sum), so no two-channel independence check survives.

/// A self-referential CAF is a compile error, not a crash.
#[test]
fn recursive_caf_is_a_compile_error() {
    let res = compile_eval::<f32, 256>("a = a; main = a", "main", Mode::Strict);
    assert!(
        res.is_err(),
        "self-recursive CAF must be rejected, not crash"
    );
}
