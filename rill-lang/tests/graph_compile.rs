use rill_lang::builtin::Registry;
use rill_lang::compile_graph;

#[test]
fn compiles_simple_graph() {
    let result = compile_graph::<f32>(
        "param osc = sin(440.0); param gain = _ * 0.5; process = osc : gain : _;",
        &Registry::<f32>::new(),
        44100.0,
    );
    assert!(result.is_ok(), "should compile simple graph");
}

#[test]
fn single_algorithm_works() {
    let result = compile_graph::<f32>("process = _ * 0.5;", &Registry::<f32>::new(), 44100.0);
    assert!(result.is_ok(), "single algorithm should compile");
}

#[test]
fn engine_executes_and_produces_output() {
    let mut engine =
        compile_graph::<f32>("process = _ * 0.5;", &Registry::<f32>::new(), 44100.0).unwrap();

    let mut output = [0.0f32; 8];
    let input = [2.0f32; 8];
    use rill_core::traits::Algorithm;
    engine.process(Some(&input), &mut output).unwrap();
    assert_eq!(output[0], 1.0, "2.0 * 0.5 = 1.0");
}

#[test]
fn param_anchor_maps_correctly() {
    let engine = compile_graph::<f32>(
        "param myGain = _ * param(\"level\", 0.5); process = _ : myGain : _;",
        &Registry::<f32>::new(),
        44100.0,
    )
    .unwrap();

    let anchor_map = engine.anchor_map();
    assert!(
        anchor_map.contains_key("myGain"),
        "param anchor 'myGain' should exist"
    );
    let inner = anchor_map.get("myGain").unwrap();
    assert!(
        inner.contains_key("level"),
        "param 'level' should be under 'myGain'"
    );
}
