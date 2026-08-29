//! Shared `ResourceRegistry` plumbing for tape resources.
//!
//! `compile_program` creates a fresh resource registry per call, so a write
//! engine and a read engine compiled separately would reference two different
//! `TapeLoop`s. `compile_program_with_resources` accepts an external registry,
//! letting the recording write head and the playback read heads share one tape.

use rill_core::buffer::{ResourceRegistry, TapeLoop};
use rill_core::builtin::Registry;
use rill_core::traits::MultichannelAlgorithm;

/// Registry with the tape write/read head built-ins.
fn reg() -> Registry<f32> {
    let mut r = Registry::new();
    rill_digital_effects::lang::tape::register_tape_builtins(&mut r);
    r
}

/// Parse DSL source into a `Program`.
fn parse(src: &str) -> rill_lang::ast::Program {
    let tokens = rill_lang::lexer::tokenize(src).unwrap();
    rill_lang::parser::parse(&tokens, src.as_bytes()).unwrap()
}

#[test]
fn shared_registry_wires_write_and_read_heads_to_one_tape() {
    let mut resources = ResourceRegistry::<f32>::new();
    resources.register_tape("tape_0", TapeLoop::<f32>::new(1024).unwrap());

    let write_src = "tape_0 = TapeLoop 1024\nmain = (_, _) :> write_head tape_0 0.5 0.3";
    let write_prog = parse(write_src);
    let mut write_engine =
        rill_lang::compile_program_with_resources(&write_prog, &reg(), 44100.0, &mut resources)
            .unwrap();

    let read_src = "tape_0 = TapeLoop 1024\nmain = read_head tape_0 0.1";
    let read_prog = parse(read_src);
    let mut read_engine =
        rill_lang::compile_program_with_resources(&read_prog, &reg(), 44100.0, &mut resources)
            .unwrap();

    // The read head reads `delay` samples *behind* the write head (0.1 s at
    // 44100 Hz = 4410 samples), past the 1024-sample tape capacity. Write
    // enough blocks to wrap the whole tape with the constant 1.0 so the read
    // head's clamped, interpolated read samples recorded data.
    let dry = [1.0f32; 64];
    let fb = [0.0f32; 64];
    let mut wout = [0.0f32; 64];
    for _ in 0..20 {
        MultichannelAlgorithm::process(&mut write_engine, &[&dry, &fb], &mut [&mut wout[..]])
            .unwrap();
    }

    let mut out = [0.0f32; 64];
    MultichannelAlgorithm::process(&mut read_engine, &[], &mut [&mut out[..]]).unwrap();
    assert!(
        out.iter().any(|v| v.abs() > 1e-6),
        "read head must see the write head's samples through the shared tape"
    );
}

#[test]
fn missing_resource_in_external_registry_is_compile_error() {
    let write_src = "tape_0 = TapeLoop 1024\nmain = (_, _) :> write_head tape_0 0.5 0.3";
    let write_prog = parse(write_src);

    let mut empty = ResourceRegistry::<f32>::new();
    let res = rill_lang::compile_program_with_resources(&write_prog, &reg(), 44100.0, &mut empty);
    assert!(
        res.is_err(),
        "a resource absent from the provided registry must be a compile error, not a silent dead engine"
    );
}
