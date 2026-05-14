use rill_core::NodeId;
use rill_core_actor::ActorSystem;
use rill_patchbay::{LfoWaveform, Servo};
use std::sync::Arc;

fn tokio_rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}

#[test]
fn test_lfo_servo_spawn() {
    let rt = tokio_rt();
    let _guard = rt.enter();
    let system = Arc::new(ActorSystem::new());
    let graph_actor = system.spawn("graph", |_| {});
    let servo = Servo::new(
        "test_lfo",
        LfoAutomaton::new("lfo", 1.0, 0.5, 0.0, LfoWaveform::Sine),
        NodeId(1),
        "cutoff",
        rill_patchbay::ParameterMapping::Linear,
        100.0,
        1000.0,
        system.clone(),
        graph_actor.actor_ref(),
    );
    assert_eq!(servo.id(), "test_lfo");
    let _actor_ref = servo.spawn(&system);
}

use rill_patchbay::LfoAutomaton;

#[test]
fn test_envelope_servo_spawn() {
    let rt = tokio_rt();
    let _guard = rt.enter();
    use rill_patchbay::EnvelopeAutomaton;
    let system = Arc::new(ActorSystem::new());
    let graph_actor = system.spawn("graph", |_| {});
    let servo = Servo::new(
        "test_env",
        EnvelopeAutomaton::adsr("env", 0.1, 0.2, 0.7, 0.3),
        NodeId(1),
        "gain",
        rill_patchbay::ParameterMapping::Linear,
        0.0,
        1.0,
        system.clone(),
        graph_actor.actor_ref(),
    );
    assert_eq!(servo.id(), "test_env");
    let _actor_ref = servo.spawn(&system);
}
