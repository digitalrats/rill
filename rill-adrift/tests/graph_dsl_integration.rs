#![cfg(feature = "lang")]

use rill_core::queues::CommandEnum;
use rill_core::traits::{Algorithm, ParamValue};
use rill_lang::compile_graph;

#[test]
fn simple_graph_osc_gain_chain() {
    let src = "process = sin(220.0) * 0.5;";
    let mut engine = compile_graph::<f32>(src, &rill_lang::builtin::Registry::new(), 44100.0)
        .expect("should compile");

    let mut output = [0.0f32; 64];
    engine.process(None, &mut output).unwrap();

    let has_signal = output.iter().any(|&v| v.abs() > 1e-6);
    assert!(has_signal, "sine oscillator should produce non-zero output");

    let all_in_range = output.iter().all(|&v| v >= -0.5 && v <= 0.5);
    assert!(all_in_range, "output should be within gain range");
}

#[test]
fn plain_process_half_input() {
    let src = "process = _ * 0.5;";
    let mut engine = compile_graph::<f32>(src, &rill_lang::builtin::Registry::new(), 44100.0)
        .expect("should compile");

    let mut output = [0.0f32; 64];
    let input = [2.0f32; 64];
    engine.process(Some(&input), &mut output).unwrap();

    assert_eq!(output[0], 1.0, "2.0 * 0.5 = 1.0");
}

#[test]
fn param_graph_accepts_graph_set_parameter() {
    let src = r#"
param myGain = _ * param("gain", 0.5);
process = _ : myGain : _;
"#;
    let mut engine = compile_graph::<f32>(src, &rill_lang::builtin::Registry::new(), 44100.0)
        .expect("should compile");

    let handle = engine.handle();
    handle.send(CommandEnum::GraphSetParameter {
        anchor: "myGain".into(),
        param: "gain".into(),
        value: ParamValue::Float(0.25),
    });

    let mut output = [0.0f32; 64];
    let input = [4.0f32; 64];
    engine.process(Some(&input), &mut output).unwrap();

    assert!(
        (output[0] - 1.0).abs() < 1e-5,
        "expected ~1.0, got {}",
        output[0]
    );
}

#[test]
fn feedback_graph_compiles_and_runs() {
    let src = "process = _ ~ _ * 0.5;";
    let result = compile_graph::<f32>(src, &rill_lang::builtin::Registry::new(), 44100.0);
    assert!(
        result.is_ok(),
        "feedback graph should compile: {:?}",
        result.err()
    );
    let mut engine = result.unwrap();
    let mut output = [0.0f32; 64];
    engine.process(Some(&[1.0f32; 64]), &mut output).unwrap();
    assert!(output.iter().all(|v| v.is_finite()));
}

#[test]
fn keep_param_compiles() {
    let src = "keep param kf = _ * 0.5; process = _ : kf : _;";
    let result = compile_graph::<f32>(src, &rill_lang::builtin::Registry::new(), 44100.0);
    assert!(
        result.is_ok(),
        "keep param should compile: {:?}",
        result.err()
    );
}
