//! The `main : IO ()` compile gate and the runner surface it protects.
//!
//! [`compile`] is the only entry that requires an `IO ()` entry: `main` is a
//! *value*, not a function, so a source whose entry is a scalar, a buffer, or
//! an `IO a` with `a` not `()` is rejected before any world exists. These pins
//! lock the gate's position (compile time, not run time), its message, and the
//! runner behaviors Task 4's world-state split must keep: the captured console
//! and the re-seat-idempotent `set_param` path between runs.

use rill_core::traits::ParamValue;
use rill_lang::compile;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::register;
use rill_lang::CompileError;

/// The compile-gate message, verbatim from the source (`lib.rs`, the
/// `ir.main_io_unit` check).
const IO_UNIT_GATE: &str = "`main` must have type `IO ()`";

/// A foreign registry carrying the DSP generators (`sine`, `square`, …).
fn ffi_with_generators() -> ForeignRegistry<f32> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    ffi
}

/// Compile `src`, requiring the gate to reject it, and return the error.
///
/// `ProgramRunner` is deliberately not `Debug` (it owns single-threaded buffer
/// state), so this matches instead of calling `expect_err`.
fn rejected(src: &str, ffi: ForeignRegistry<f32>) -> CompileError {
    match compile::<f32, 256>(src, ffi) {
        Ok(_) => panic!("the compile gate must reject `{src}`"),
        Err(err) => err,
    }
}

#[test]
fn a_scalar_main_is_rejected_at_compile() {
    // `main` must be an `IO ()` value: a plain `Double` is not one.
    let err = rejected("main = 1.0 + 2.0", ForeignRegistry::new());

    let message = err.to_string();
    assert!(
        matches!(err, CompileError::Unsupported(_)),
        "the gate reports `Unsupported`, got: {err:?}"
    );
    assert!(
        message.contains(IO_UNIT_GATE),
        "expected the `IO ()` gate message, got: {message}"
    );
}

#[test]
fn a_buffer_main_is_rejected_at_compile() {
    // A `FixedBuffer Double` entry is well-typed but is not an `IO ()` action.
    // The gate fires in `compile` before any `WorldState::new`, so this pins
    // the GATE rather than an unresolvable built-in: registration is never
    // observable here, and the entry type alone rejects the program.
    let err = rejected("main = sine 440.0 0.5 0.0", ffi_with_generators());

    let message = err.to_string();
    assert!(
        matches!(err, CompileError::Unsupported(_)),
        "the gate reports `Unsupported`, got: {err:?}"
    );
    assert!(
        message.contains(IO_UNIT_GATE),
        "expected the `IO ()` gate message, got: {message}"
    );
}

#[test]
fn an_io_unit_main_compiles_and_writes_to_captured_stdout() {
    // `write`/`stdout` are builtin language effects — no foreign registration
    // is needed for them.
    let mut runner = compile::<f32, 256>(
        r#"main = do { write (Just "a") stdout; return () };"#,
        ForeignRegistry::new(),
    )
    .expect("an `IO ()` main must compile with an empty foreign registry");

    runner.run().expect("the program must run to completion");
    assert_eq!(runner.stdout(), ["a"], "stdout is captured by the runner");
    assert_eq!(runner.stderr(), [] as [String; 0], "stderr stays empty");
}

#[test]
fn an_effect_final_main_runs_and_a_non_unit_result_is_rejected() {
    // (a) An effect-final `do` block: the final action's result IS `()`, so the
    // static `IO ()` type holds and the block runs.
    let mut runner = compile::<f32, 256>(
        r#"main = do { write (Just "ok") stdout };"#,
        ForeignRegistry::new(),
    )
    .expect("an effect-final `IO ()` main must compile");
    runner
        .run()
        .expect("an effect-final program must run to completion");
    assert_eq!(runner.stdout(), ["ok"]);

    // (b) `do { …; return 1.0 }` types as `IO Double`, so the gate rejects it at
    // COMPILE time — the world is never built and the program never runs. The
    // runtime latch in `run_main_io` (`the final action's result is not ()`) is
    // belt-and-braces for a payload the static type cannot classify; through
    // the public `compile` surface this source cannot reach it.
    let err = rejected(
        r#"main = do { write (Just "a") stdout; return 1.0 };"#,
        ForeignRegistry::new(),
    );

    let message = err.to_string();
    assert!(
        matches!(err, CompileError::Unsupported(_)),
        "the gate reports `Unsupported`, got: {err:?}"
    );
    assert!(
        message.contains(IO_UNIT_GATE),
        "expected the `IO ()` gate message, got: {message}"
    );
}

#[test]
fn a_set_param_between_runs_is_observed_by_the_second_run() {
    // One world, two runs: the seat is idempotent for the parameter store
    // (declared defaults are seeded absent-only), so a `set_param` written
    // between runs is the second run's value.
    let mut runner = compile::<f32, 256>(
        r#"main = do { write (Just (show_double ?level)) stdout; return () };"#,
        ForeignRegistry::new(),
    )
    .expect("compile");

    runner.run().expect("first run");
    assert_eq!(
        runner.stdout(),
        ["0.0"],
        "the first run observes the declared default"
    );

    runner
        .world_mut()
        .set_param("level", ParamValue::Float(0.5));
    runner.run().expect("second run");

    assert_eq!(
        runner.stdout(),
        ["0.0", "0.5"],
        "the console is cumulative and the second run observes the value set \
         between runs (the re-seat must not overwrite it)"
    );
    assert_eq!(
        runner.world().param("level"),
        Some(ParamValue::Float(0.5)),
        "the world's name-addressed store keeps the value across the re-seat"
    );
}
