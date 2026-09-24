//! CAF (closed top-level definition) free-variable integration tests.
//!
//! Behavioral, end-to-end coverage of closed top-level definitions through the
//! public `compile_with` entry point. The core property under test: a closed
//! top-level stateful definition referenced from two places lowers to ONE
//! shared instance, and its observable state is shared across every reference.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use rill_core::builtin::BlockBuiltin;
use rill_core::traits::{Algorithm, MultichannelAlgorithm, ProcessResult};
use rill_lang::builtin::{BuiltinKind, BuiltinSig, Registry};
use rill_lang::{compile, compile_with};

/// A real block oscillator (0 signal in, 1 out; params freq/amp/phase) whose
/// phase accumulates per sample.
///
/// Each `process` call bumps a harness-owned `AtomicUsize` so tests can observe
/// how many instances a compiled program actually runs per tick: a single
/// shared CAF instance bumps it once, a duplicated build would bump it once per
/// channel.
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
fn sine_registry() -> (Registry<f32>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = Registry::<f32>::new();
    let shared = calls.clone();
    registry.register_block(
        BuiltinSig::simple("sine", 0, 1, 3, BuiltinKind::Block),
        move |params, sample_rate| Box::new(SineOsc::new(params, sample_rate, shared.clone())),
    );
    (registry, calls)
}

/// `osc = sine 440.0 0.5 0.0; main = osc , osc` must be ONE shared oscillator.
///
/// The program exposes two outputs fed by the same phase accumulator: the call
/// counter advances by exactly one per tick (a duplicated build would advance
/// by two), and both channels must agree — even after the shared accumulator
/// advances into the second tick. `ch0 == ch1` asserts that agreement, not
/// causation: two independent lockstep instances would also agree, so the call
/// counter is the real sharing discriminator.
#[test]
fn shared_oscillator_runs_one_instance_per_tick() {
    let (registry, calls) = sine_registry();
    let mut prog = compile_with::<f32>(
        "osc = sine 440.0 0.5 0.0; main = osc , osc",
        &registry,
        44100.0,
    )
    .expect("program with a shared CAF compiles");

    assert_eq!(prog.num_inputs(), 0);
    assert_eq!(prog.num_outputs(), 2);

    let mut ch0 = [0.0f32; 8];
    let mut ch1 = [0.0f32; 8];
    let mut outs = [&mut ch0[..], &mut ch1[..]];
    MultichannelAlgorithm::process(&mut prog, &[], &mut outs).expect("first tick");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "one shared sine instance must run once per tick"
    );
    assert_eq!(
        ch0, ch1,
        "both outputs come from the same phase accumulator"
    );

    // A second tick continues the shared phase; the samples must differ from
    // the first block (proving the accumulator advances) while still running
    // exactly one instance.
    let mut ch0b = [0.0f32; 8];
    let mut ch1b = [0.0f32; 8];
    let mut outs = [&mut ch0b[..], &mut ch1b[..]];
    MultichannelAlgorithm::process(&mut prog, &[], &mut outs).expect("second tick");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(ch0b, ch1b);
    assert_ne!(
        ch0, ch0b,
        "the shared phase accumulator advances across ticks"
    );
}

/// An open block (macro) inlined twice stays two independent stateful paths.
///
/// `integ = + ~ _` is a macro (it takes a signal input), so referencing it
/// twice compiles two independent integrator instances — one per input channel.
/// Feeding each channel a different stream must integrate that stream alone.
#[test]
fn open_block_inlined_twice_stays_independent() {
    let mut registry = Registry::<f32>::new();
    rill_core_dsp::lang::register::register_lang_builtins(&mut registry);
    let mut prog = compile_with::<f32>("integ = + ~ _; main = integ , integ", &registry, 44100.0)
        .expect("open block compiles");

    assert_eq!(prog.num_inputs(), 2);
    assert_eq!(prog.num_outputs(), 2);

    // Each `integ` site owns its own input channel and its own running sum:
    // feeding different streams must produce channel-specific integration.
    let in0 = [1.0f32; 4];
    let in1 = [2.0f32; 4];
    let mut ch0 = [0.0f32; 4];
    let mut ch1 = [0.0f32; 4];
    let mut outs = [&mut ch0[..], &mut ch1[..]];
    MultichannelAlgorithm::process(&mut prog, &[&in0, &in1], &mut outs).expect("tick");

    assert_eq!(
        ch0,
        [1.0, 2.0, 3.0, 4.0],
        "channel 0 integrates its own stream"
    );
    assert_eq!(
        ch1,
        [2.0, 4.0, 6.0, 8.0],
        "channel 1 integrates its own stream"
    );
    assert!(ch0.iter().all(|v| v.is_finite()), "output must stay finite");
}

/// A self-referential CAF is a compile error, not a crash.
#[test]
fn recursive_caf_is_a_compile_error() {
    let res = compile::<f32>("a = a; main = a");
    assert!(
        res.is_err(),
        "self-recursive CAF must be rejected, not crash"
    );
}

/// A user-defined function can capture a shared free variable.
///
/// `voice gain = osc * gain` references the closed CAF `osc`; both calls
/// `voice 0.5` / `voice 0.7` must share ONE sine instance (counter advances by
/// one per tick) while applying their own gain to the same shared phase.
#[test]
fn user_function_captures_shared_free_variable() {
    let (registry, calls) = sine_registry();
    let src = "osc = sine 440.0 0.5 0.0; voice gain = osc * gain; main = voice 0.5 , voice 0.7";
    let mut prog = compile_with::<f32>(src, &registry, 44100.0)
        .expect("user function capturing a CAF compiles");

    assert_eq!(prog.num_outputs(), 2);

    let mut ch0 = [0.0f32; 8];
    let mut ch1 = [0.0f32; 8];
    let mut outs = [&mut ch0[..], &mut ch1[..]];
    MultichannelAlgorithm::process(&mut prog, &[], &mut outs).expect("tick");

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "the captured CAF is one shared instance"
    );
    assert!(
        ch0.iter().any(|v| *v != 0.0),
        "the shared sine must produce a non-zero signal"
    );
    // ch0 = phase * 0.5, ch1 = phase * 0.7 → ch1 = 1.4 * ch0.
    for (a, b) in ch0.iter().zip(ch1.iter()) {
        let expected = a * (0.7 / 0.5);
        assert!(
            (b - expected).abs() < 1e-4,
            "gain 0.7 channel must scale the gain 0.5 channel"
        );
    }
}
