use rill_core_actor::ActorSystem;
use rill_lang::builtin::Registry;
use rill_lang::compile_graph;

#[test]
fn compiles_simple_graph() {
    let system = ActorSystem::new();
    let result = compile_graph::<f32, 64>(
        "param osc = sin(440.0); param gain = _ * 0.5; process = osc : gain : _;",
        &Registry::<f32>::new(),
        44100.0,
        &system,
    );
    assert!(result.is_ok(), "should compile simple graph");
}

#[test]
fn single_algorithm_falls_back() {
    let system = ActorSystem::new();
    let result = compile_graph::<f32, 64>(
        "process = _ * 0.5;",
        &Registry::<f32>::new(),
        44100.0,
        &system,
    );
    assert!(
        result.is_ok(),
        "single algorithm should fall back to inline mode"
    );
}
