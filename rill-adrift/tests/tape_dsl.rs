//! DSL tape loop constructor + symbolic resource reference tests.
use rill_adrift::lang_builtins::full_registry_f32;

use rill_lang::compile_graph;

#[test]
fn tape_loop_constructor_and_read_head_compile() {
    let reg = full_registry_f32();
    let src = "tape_0 = TapeLoop 1024\nmain = read_head tape_0 0.1";
    let mut engine = compile_graph::<f32>(src, &reg, 44100.0).unwrap();
    let mut out = [0.0f32; 64];
    engine.process_tick(&[], &mut [&mut out[..]], 0).unwrap();
    assert!(out.iter().all(|v| v.is_finite()));
}

#[test]
fn write_head_compiles_and_runs() {
    let reg = full_registry_f32();
    let src = "tape_0 = TapeLoop 1024\nmain = _ : write_head tape_0 0.5 0.3";
    let mut engine = compile_graph::<f32>(src, &reg, 44100.0).unwrap();
    let input = [1.0f32; 64];
    let mut out = [0.0f32; 64];
    engine
        .process_tick(&[&input[..]], &mut [&mut out[..]], 0)
        .unwrap();
    assert!(out.iter().all(|v| v.is_finite()));
}

#[test]
fn undeclared_resource_is_compile_error() {
    let reg = full_registry_f32();
    let src = "main = read_head missing_tape 0.1";
    assert!(compile_graph::<f32>(src, &reg, 44100.0).is_err());
}
