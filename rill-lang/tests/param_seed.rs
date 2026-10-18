//! Task 3: the world's parameter store is addressed **by name**.
//!
//! `world.params` is a [`rill_core::Params`]: `?sample_rate` IS the
//! `Params::sample_rate` field (never a map entry, never seeded), every other
//! `?name` lives in `Params::parameters`. Seating a world seeds the declared
//! parameter **defaults** only where the caller has not already supplied a
//! value, so a pre-seeded world survives the seat (initial-state injection).
//!
//! `smooth` reads the world's sample rate at run time, so the coefficient is
//! no longer baked into the IR: the parity test pins the runtime coefficient
//! against the formula the compiler used to bake (`a = 1 - exp(-1/(τ·sr))`,
//! computed in f64), and the zero-rate case must degenerate to identity
//! (no NaN, no infinities).
//!
//! The IR is built through the public pipeline stages (`lexer` → `parser` →
//! `types::infer` → `reduce` → `lower`) because the crate-private
//! `compile_to_ir_entry` helper is not reachable from an integration test; this
//! is the same sequence used by `tests/world_seat.rs`.

use rill_core::traits::ParamValue;
use rill_lang::arena::Value;
use rill_lang::builtin::Registry;
use rill_lang::compile_runner;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;
use rill_lang::program::{RillProgram, WorldConfig, WorldState};
use rill_lang::register;
use rill_lang::{test_eval, Ir};

/// Block size used by every test (matches the crate default).
const BUF: usize = 256;

/// Compile `src` to an `Ir` with entry `"main"` (the `world_seat` pipeline).
fn compile_ir(src: &str) -> Ir {
    let tokens = rill_lang::lexer::tokenize(src).expect("lex");
    let mut program = rill_lang::parser::parse(&tokens, src.as_bytes()).expect("parse");
    program.mode = Mode::Strict;
    let mut typed =
        rill_lang::types::infer::infer_program_with(&program, Mode::Strict).expect("infer");
    typed.program =
        rill_lang::reduce::reduce_with_cafs_mode(&typed.program, &typed.cafs, typed.mode);
    rill_lang::lower::lower_with_cafs_mode(&typed, &typed.cafs, typed.mode).expect("lower")
}

/// A world seated for `src` at `sample_rate`, with no built-ins registered.
fn seated_world(src: &str, sample_rate: f32) -> WorldState<f32, BUF> {
    let ir = compile_ir(src);
    let ffi = ForeignRegistry::<f32>::new();
    let mut world = WorldState::<f32, BUF>::new(WorldConfig::default());
    world.seat(&ir, &ffi, sample_rate).expect("seat");
    world
}

/// Evaluate `expr` once at `sample_rate` with the DSP generators registered.
fn eval_block(expr: &str, sample_rate: f32) -> RillProgram<f32, BUF> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    test_eval::<f32, BUF>("", expr, &ffi, sample_rate)
        .unwrap_or_else(|e| panic!("`{expr}` must compile and run: {e}"))
}

/// Copy the run's whole-buffer result (a `Value::Buffer`) out of the world.
fn block_of(prog: &RillProgram<f32, BUF>) -> Vec<f32> {
    let r = prog.result().expect("the run must produce a result");
    let mut out = vec![0.0f32; BUF];
    assert!(
        prog.copy_buffer(r, &mut out),
        "the result must be a whole-buffer value"
    );
    out
}

/// The `smooth` coefficient, computed with the same operation order the
/// runtime coefficient path uses (`a = 1 - exp(-1 / ((ms/1000) * sr))`).
fn runtime_coefficient(ms: f32, sample_rate: f32) -> f32 {
    let tau_ms = ms / 1000.0;
    let denominator = tau_ms * sample_rate;
    let inverse = -1.0f32 / denominator;
    1.0f32 - inverse.exp()
}

// ---------------------------------------------------------------------------
// Pre-seeding and absent-only seating (FU-3)
// ---------------------------------------------------------------------------

#[test]
fn a_param_preseeded_before_seat_survives_the_seat() {
    let ir = compile_ir("main = ?level * 1.0");
    let ffi = ForeignRegistry::<f32>::new();
    let mut world = WorldState::<f32, BUF>::new(WorldConfig::default());

    world.set_param("level", ParamValue::Float(0.5));
    world.seat(&ir, &ffi, 44_100.0).expect("seat");

    assert_eq!(
        world.param("level"),
        Some(ParamValue::Float(0.5)),
        "the seat seeds declared defaults only where no value exists — a \
         pre-seeded parameter must survive (initial-state injection)"
    );
    assert_eq!(
        world.params_meta()[0].default,
        0.0,
        "the metadata still declares the IR default"
    );
}

#[test]
fn world_config_params_survive_the_seat() {
    let ir = compile_ir("main = ?level * 1.0");
    let config = WorldConfig::<f32> {
        params: rill_core::Params::new(48_000.0).with("level", ParamValue::Float(0.25)),
        ..Default::default()
    };
    let mut world = WorldState::<f32, BUF>::new(config);

    world
        .seat(&ir, &ForeignRegistry::<f32>::new(), 44_100.0)
        .expect("seat");

    assert_eq!(
        world.param("level"),
        Some(ParamValue::Float(0.25)),
        "WorldConfig::params is an initial-state seed and must survive seating"
    );
    assert_eq!(
        world.params.sample_rate, 44_100.0,
        "the seat argument is the transition's sample-rate seed"
    );
}

#[test]
fn a_declared_param_without_a_preseed_takes_its_default() {
    let world = seated_world("main = ?level * 1.0", 44_100.0);

    assert_eq!(
        world.param("level"),
        Some(ParamValue::Float(0.0)),
        "an absent parameter is seeded from the IR default"
    );
    assert_eq!(
        world.param("never_declared"),
        None,
        "an undeclared name is simply absent from the store"
    );
}

// ---------------------------------------------------------------------------
// `?sample_rate` is the world's `Params::sample_rate` field
// ---------------------------------------------------------------------------

#[test]
fn sample_rate_is_the_world_field_and_never_a_map_entry() {
    let world = seated_world("main = ?sample_rate", 44_100.0);

    assert_eq!(
        world.params.sample_rate, 44_100.0,
        "the seat seeds the sample-rate field"
    );
    assert_eq!(
        world.param("sample_rate"),
        Some(ParamValue::Float(44_100.0))
    );
    assert!(
        !world.params.parameters.contains_key("sample_rate"),
        "`sample_rate` is owned by the field, never by the named map"
    );
}

#[test]
fn a_run_reads_sample_rate_from_the_world_field() {
    let ffi = ForeignRegistry::<f32>::new();
    let prog = test_eval::<f32, BUF>("", "?sample_rate", &ffi, 48_000.0).expect("eval");

    let r = prog.result().expect("the run must produce a result");
    assert_eq!(
        prog.arena().get(r),
        Some(&Value::Float(48_000.0)),
        "`?sample_rate` must read the world's field value"
    );
}

#[test]
fn set_param_sample_rate_writes_the_field() {
    let mut world = seated_world("main = ?sample_rate", 44_100.0);

    world.set_param("sample_rate", ParamValue::Float(96_000.0));

    assert_eq!(world.params.sample_rate, 96_000.0);
    assert_eq!(
        world.param("sample_rate"),
        Some(ParamValue::Float(96_000.0))
    );
    assert!(
        !world.params.parameters.contains_key("sample_rate"),
        "writing the rate must not create a named map entry"
    );
}

// ---------------------------------------------------------------------------
// `smooth` computes its coefficient from the world rate at run time
// ---------------------------------------------------------------------------

#[test]
fn smooth_matches_the_old_baked_coefficient_at_48k() {
    // A square at 0 Hz holds a constant +1.0 across the block, so the seeded
    // input is a known fixed waveform.
    let input = block_of(&eval_block("square 0.0 1.0 0.0", 48_000.0));
    assert_eq!(input[0], 1.0, "the seeded square is a constant +1.0 block");
    let got = block_of(&eval_block("smooth (square 0.0 1.0 0.0) 5.0", 48_000.0));

    // The block-feedback slot starts at zero and is only swapped in at tick
    // end, so one run evaluates exactly `y = 0 + a*(x - 0)` per sample: the
    // coefficient `a` applied to the block. Replay it with the runtime
    // coefficient (same operation order) and require exact sample equality.
    let a = runtime_coefficient(5.0, 48_000.0);
    for (i, &x) in input.iter().enumerate() {
        let y = 0.0f32 + a * (x - 0.0f32);
        assert_eq!(
            got[i], y,
            "sample {i}: the runtime coefficient path must apply \
             `a = 1 - exp(-1/(τ·sr))` to the block (x = {x})"
        );
    }

    // Parity with the OLD bake: the compiler used to evaluate the coefficient
    // in f64 at build time and broadcast it. The two agree to within a couple
    // of f32 ULPs (`a ≈ 4.16e-3`): the runtime path computes the same formula
    // at the scalar type's precision.
    let baked = (1.0 - (-1.0f64 / ((5.0f64 / 1000.0) * 48_000.0)).exp()) as f32;
    let measured = got[0] / input[0];
    assert!(
        (measured - baked).abs() < 1e-6,
        "runtime coefficient {measured:e} must match the old baked {baked:e}"
    );
    assert!(got.iter().all(|v| v.is_finite()));
}

#[test]
fn smooth_with_a_zero_sample_rate_is_identity() {
    // `sample_rate = 0.0` (the default world rate) must degenerate to
    // `a = 1.0` — identity — instead of dividing by zero into NaN/inf.
    // (A zero-rate square oscillator still yields a finite block, so the
    // comparison pins the OPERATOR's behavior, not the generator's.)
    let input = block_of(&eval_block("square 0.0 1.0 0.0", 0.0));
    let got = block_of(&eval_block("smooth (square 0.0 1.0 0.0) 5.0", 0.0));

    assert!(got.iter().all(|v| v.is_finite()), "no NaN/inf may escape");
    assert_eq!(
        got, input,
        "a zero-rate world makes the smoother the identity (`a = 1.0`)"
    );
}

#[test]
fn smooth_with_a_negative_sample_rate_is_identity() {
    // `sr < 0` must hit the same `sr <= 0` identity guard as `sr == 0`. Without
    // it the denominator goes negative, `-1/den` becomes positive,
    // `exp(+240)` overflows to `+inf` and `a = 1 - inf = -inf`, so the smoother
    // would emit non-finite output. The guard is clamped in the emitted
    // sequence: `max((ms/1000)·sr, 0)`, which degenerates both cases to `a = 1`.
    let input = block_of(&eval_block("square 0.0 1.0 0.0", -48_000.0));
    let got = block_of(&eval_block("smooth (square 0.0 1.0 0.0) 5.0", -48_000.0));

    assert!(got.iter().all(|v| v.is_finite()), "no NaN/inf may escape");
    assert_eq!(
        got, input,
        "a negative-rate world makes the smoother the identity (`a = 1.0`)"
    );
}

// ---------------------------------------------------------------------------
// One world, several runs: params persist and re-reads observe the new value
// ---------------------------------------------------------------------------

#[test]
fn a_seated_world_observes_param_changes_between_runs() {
    let mut runner = compile_runner::<f32, BUF>(
        r#"main = do { write (Just (show_float ?level)) stdout; return () };"#,
        &Registry::new(),
        44_100.0,
    )
    .expect("compile");

    runner.run().expect("first run");
    runner
        .program_mut()
        .set_param("level", ParamValue::Float(0.5));
    runner.run().expect("second run");

    assert_eq!(
        runner.stdout(),
        ["0.0", "0.5"],
        "the second run of the SAME world must observe the value set between runs"
    );
}
