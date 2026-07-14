//! Integration tests for CompiledGraphEngine parameter propagation.
//!
//! Verifies SetParameter routing through the engine's mailbox → drain_mailbox
//! → param_maps → NodeClosure::set_param → RillProgram chain.

use rill_adrift::lang_builtins::full_registry;
use rill_core::queues::SetParameter;
use rill_core::traits::{ParamValue, ParameterId};
use rill_lang::compile_graph;

/// Compile DSL, send SetParameter, call process_tick, return output.
fn run_with_param(
    src: &str,
    param_name: &str,
    param_value: ParamValue,
    input: &[&[f32]],
    buf_size: usize,
) -> Vec<f32> {
    let reg = full_registry::<f32>();
    let mut engine = compile_graph::<f32>(src, &reg, 44100.0).unwrap();

    let sp = SetParameter::new(
        String::new(),
        ParameterId::new(param_name).unwrap(),
        param_value,
        rill_core::queues::SignalOrigin::Manual,
    );
    engine
        .handle()
        .send(rill_core::queues::CommandEnum::SetParameter(sp));

    let mut output = vec![0.0f32; buf_size];
    let input_slices: Vec<&[f32]> = input.to_vec();
    engine
        .process_tick(&input_slices, &mut [&mut output[..]], 0)
        .unwrap();

    output
}

// ---------------------------------------------------------------------------
// main function parameters
// ---------------------------------------------------------------------------

#[test]
fn main_arg_param_affects_output() {
    // main g = _ * g
    // Default: g = 0.0 (parameters default to 0.0 when compiled via rill-lang)
    // After SetParameter("g", 2.0): output = input * 2.0
    let signal = [1.0f32; 256];
    let out = run_with_param(
        "main g = _ * g",
        "g",
        ParamValue::Float(2.0),
        &[&signal[..]],
        256,
    );

    for (i, &v) in out.iter().enumerate() {
        assert!((v - 2.0).abs() < 1e-6, "sample {i}: expected 2.0, got {v}");
    }
}

// ---------------------------------------------------------------------------
// ?name actor parameters
// ---------------------------------------------------------------------------

#[test]
fn actor_param_default_applies() {
    // main = _ * ?gain=0.25
    // No SetParameter — default 0.25 should apply
    let reg = full_registry::<f32>();
    let mut engine = compile_graph::<f32>("main = _ * ?gain=0.25", &reg, 44100.0).unwrap();

    let signal = [2.0f32; 256];
    let mut output = vec![0.0f32; 256];
    engine
        .process_tick(&[&signal[..]], &mut [&mut output[..]], 0)
        .unwrap();

    for &v in &output {
        assert!(
            (v - 0.5).abs() < 1e-6,
            "default ?gain=0.25, input 2.0 → expected 0.5, got {v}"
        );
    }
}

#[test]
fn actor_param_set_param_affects_output() {
    // main = _ * ?gain=0.25
    // After SetParameter("gain", 4.0): output = input * 4.0
    let signal = [2.0f32; 256];
    let out = run_with_param(
        "main = _ * ?gain=0.25",
        "gain",
        ParamValue::Float(4.0),
        &[&signal[..]],
        256,
    );

    for &v in &out {
        assert!(
            (v - 8.0).abs() < 1e-6,
            "?gain=4.0, input 2.0 → expected 8.0, got {v}"
        );
    }
}

#[test]
fn actor_param_no_default_applies() {
    // main = _ * ?gain
    // No default — should be 0.0, so output = 0.0
    let reg = full_registry::<f32>();
    let mut engine = compile_graph::<f32>("main = _ * ?gain", &reg, 44100.0).unwrap();

    let signal = [1.0f32; 64];
    let mut output = vec![0.0f32; 64];
    engine
        .process_tick(&[&signal[..]], &mut [&mut output[..]], 0)
        .unwrap();

    for &v in &output {
        assert!(
            (v - 0.0).abs() < 1e-6,
            "?gain default is 0.0, expected 0.0, got {v}"
        );
    }
}

#[test]
fn actor_param_persists_across_ticks() {
    // main = _ * ?gain=1.0
    // SetParameter("gain", 3.0) → tick 1
    // No SetParameter → tick 2 — gain should still be 3.0
    let reg = full_registry::<f32>();
    let mut engine = compile_graph::<f32>("main = _ * ?gain=1.0", &reg, 44100.0).unwrap();

    // Tick 1: set gain to 3.0
    let sp = SetParameter::new(
        String::new(),
        ParameterId::new("gain").unwrap(),
        ParamValue::Float(3.0),
        rill_core::queues::SignalOrigin::Manual,
    );
    engine
        .handle()
        .send(rill_core::queues::CommandEnum::SetParameter(sp));

    let signal = [1.0f32; 64];
    let mut out1 = vec![0.0f32; 64];
    engine
        .process_tick(&[&signal[..]], &mut [&mut out1[..]], 0)
        .unwrap();
    for &v in &out1 {
        assert!((v - 3.0).abs() < 1e-6, "tick 1: expected 3.0, got {v}");
    }

    // Tick 2: gain should persist (no new SetParameter)
    let mut out2 = vec![0.0f32; 64];
    engine
        .process_tick(&[&signal[..]], &mut [&mut out2[..]], 0)
        .unwrap();
    for &v in &out2 {
        assert!((v - 3.0).abs() < 1e-6, "tick 2: expected 3.0, got {v}");
    }
}

// ---------------------------------------------------------------------------
// Source node (no signal input) — verify output & param routing
// ---------------------------------------------------------------------------

#[test]
fn source_node_produces_output() {
    // A source node with no signal inputs should produce audio.
    // Use a simple DSL that generates a constant tone.
    let reg = full_registry::<f32>();
    let mut engine = compile_graph::<f32>("main = 0.5", &reg, 44100.0).unwrap();

    let mut output = vec![0.0f32; 64];
    engine.process_tick(&[], &mut [&mut output[..]], 0).unwrap();

    for &v in &output {
        assert!((v - 0.5).abs() < 1e-6, "source node: expected 0.5, got {v}");
    }
}

#[test]
fn source_node_with_param_routing() {
    // Source with actor param: SetParameter should affect output.
    let signal = [0.0f32; 64]; // ignored for source
    let out = run_with_param(
        "main = ?value=0.5",
        "value",
        ParamValue::Float(2.0),
        &[],
        64,
    );

    for &v in &out {
        assert!(
            (v - 2.0).abs() < 1e-6,
            "source with param: expected 2.0, got {v}"
        );
    }
}

#[test]
fn source_node_without_param_produces_default() {
    // Source with actor param default: no SetParameter, should use default.
    let reg = full_registry::<f32>();
    let mut engine = compile_graph::<f32>("main = ?value=0.75", &reg, 44100.0).unwrap();

    let mut output = vec![0.0f32; 64];
    engine.process_tick(&[], &mut [&mut output[..]], 0).unwrap();

    for &v in &output {
        assert!(
            (v - 0.75).abs() < 1e-6,
            "source with default param: expected 0.75, got {v}"
        );
    }
}
