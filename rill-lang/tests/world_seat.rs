//! `WorldState` seating: the world assembles its structural tables from the
//! compiled IR layout (never from config guesses) and reports unresolvable
//! built-in names at seat time — before any run can reach an effect.
//!
//! The IR is built through the public pipeline stages (`lexer` → `parser` →
//! `types::infer` → `reduce` → `lower`) because `compile_to_ir_entry` is a
//! crate-private helper; this is the same sequence it runs for entry `"main"`
//! (the pipeline skips only resource extraction, which these sources do not
//! use).

use rill_lang::ffi::ForeignRegistry;
use rill_lang::mode::Mode;
use rill_lang::program::{WorldConfig, WorldState};
use rill_lang::Ir;

/// Compile `src` to an `Ir` with the given evaluation mode, entry `"main"`.
fn compile_ir(src: &str, mode: Mode) -> Ir {
    let tokens = rill_lang::lexer::tokenize(src).expect("lex");
    let mut program = rill_lang::parser::parse(&tokens, src.as_bytes()).expect("parse");
    program.mode = mode;
    let mut typed = rill_lang::types::infer::infer_program_with(&program, mode).expect("infer");
    typed.program =
        rill_lang::reduce::reduce_with_cafs_mode(&typed.program, &typed.cafs, typed.mode);
    rill_lang::lower::lower_with_cafs_mode(&typed, &typed.cafs, typed.mode).expect("lower")
}

#[test]
fn seat_sizes_structural_tables_from_the_ir() {
    let ir = compile_ir("main = 1.0", Mode::Complete);
    let mut world = WorldState::<f32, 256>::new(WorldConfig {
        ffi: Some(ForeignRegistry::<f32>::new()),
        ..Default::default()
    });

    world
        .seat(&ir)
        .expect("seat of a builtin-free program needs no registrations");

    assert_eq!(
        world.value_regs_len(),
        ir.num_value_regs + ir.max_call_regs,
        "the register store is the program's registers plus the call scratch"
    );
    assert_eq!(
        world.block_state_len(),
        ir.state.block_state_slots,
        "one block-feedback slot per IR block-state slot"
    );
    assert_eq!(
        world.buffers_len(),
        ir.num_buffers,
        "one buffer-table region per IR buffer"
    );
}

#[test]
fn seat_sizes_the_arena_from_the_ir() {
    let ir = compile_ir("main = 1.0", Mode::Complete);
    let mut world = WorldState::<f32, 256>::new(WorldConfig::default());

    world.seat(&ir).expect("seat");

    // The arena is sized EXACTLY from the IR's build-time capacity bound — no
    // config page size, no constant. (`ChunkedVec`'s internal page is
    // `capacity.max(1).next_power_of_two()`; it is not observable from outside
    // `rill-core`, so this pins the logical capacity that the page is derived
    // from.)
    assert_eq!(
        world.arena().capacity(),
        ir.value_state.capacity,
        "arena capacity must come from the compiled IR"
    );
    assert!(
        ir.value_state.capacity > 0,
        "the compiled program needs at least one value slot"
    );
}

#[test]
fn unregistered_foreign_builtin_fails_at_seat() {
    // A `foreign`-declared name that the registry does not carry must fail when
    // the world is seated, not at some later run.
    let ir = compile_ir(
        r"
        foreign gain : Double -> FixedBuffer Double -> FixedBuffer Double;
        main = gain 0.5 (square 0.0 1.0 0.0);
        ",
        Mode::Complete,
    );
    assert!(
        ir.builtins.iter().any(|b| b.name == "gain"),
        "the declaration must lower to a builtin instance"
    );

    let mut world = WorldState::<f32, 256>::new(WorldConfig {
        ffi: Some(ForeignRegistry::<f32>::new()),
        ..Default::default()
    });
    let err = world
        .seat(&ir)
        .expect_err("an unregistered foreign built-in must fail at seat");

    assert!(
        matches!(err, rill_lang::CompileError::Unsupported(_)),
        "seat must report the unresolvable built-in as a CompileError, got: {err:?}"
    );
}
