//! Runtime error surfaces of the world: an unresolvable built-in name is a
//! **seat-time** [`CompileError`], and `copy_buffer` reports a non-buffer result
//! as `false`.
//!
//! Lowering never resolves built-in names — it records a `BuiltinInstance` per
//! call site — so a program declaring a `foreign` function the world's registry
//! does not carry compiles cleanly and fails only when `seat` binds the
//! instance. These pins lock that boundary and the buffer-copy inspectability
//! contract (`WorldState::copy_buffer`).

use rill_lang::arena::Value;
use rill_lang::compile_eval;
use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;
use rill_lang::program::{WorldConfig, WorldState};
use rill_lang::{register, CompileError};

/// A world with the DSP generators registered (so only the deliberately
/// unregistered name is unresolvable).
fn world_with_generators() -> WorldState<f32, 256> {
    let mut ffi = ForeignRegistry::<f32>::new();
    register::register_foreign_generators(&mut ffi);
    WorldState::<f32, 256>::new(WorldConfig {
        ffi: Some(ffi),
        ..Default::default()
    })
}

#[test]
fn an_unknown_builtin_compiles_but_fails_at_seat() {
    // `mystery` is declared `foreign` but never registered. `square` IS
    // registered by `register_foreign_generators`, so the failure can only be
    // the unknown name — not a missing generator.
    let defs = "foreign mystery : Float -> FixedBuffer f32 -> FixedBuffer f32;";
    let ir = compile_eval::<f32, 256>(defs, "mystery 0.5 (square 0.0 1.0 0.0)", Mode::Complete)
        .expect("lowering does not resolve built-in names, so this must compile");
    assert!(
        ir.builtins.iter().any(|b| b.name == "mystery"),
        "the foreign declaration must lower to a built-in instance"
    );

    let mut world = world_with_generators();
    let err = rill_lang::test_eval(&ir, &mut world)
        .expect_err("seating a world must reject the unresolvable built-in");

    let message = err.to_string();
    assert!(
        matches!(err, CompileError::Unsupported(_)),
        "the unknown built-in is a seat-time `Unsupported`, got: {err:?}"
    );
    assert!(
        message.contains("mystery") && message.contains("not registered"),
        "expected the unresolvable-built-in message for `mystery`, got: {message}"
    );
}

#[test]
fn copy_buffer_is_false_and_writes_nothing_for_a_scalar_result() {
    // A `Float` run result is not a whole-buffer value: `copy_buffer` reports
    // `false` and leaves the destination untouched (the caller distinguishes
    // "scalar result" from "zero-filled block").
    let ir = compile_eval::<f32, 256>("", "1.0 + 2.0", Mode::Strict).expect("compile");
    let mut world = WorldState::<f32, 256>::new(WorldConfig::default());
    rill_lang::test_eval(&ir, &mut world).expect("eval");

    let r = world.result().expect("the run must produce a result");
    assert_eq!(
        world.arena().get(r),
        Some(&Value::Float(3.0)),
        "the pinned result is a scalar, not a buffer"
    );

    let mut dst = [7.0f32; 4];
    assert!(
        !world.copy_buffer(r, &mut dst),
        "`copy_buffer` must report `false` for a non-buffer result"
    );
    assert_eq!(
        dst, [7.0f32; 4],
        "a rejected copy must not touch the destination"
    );
}
