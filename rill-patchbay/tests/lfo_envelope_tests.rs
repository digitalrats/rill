use rill_core::queues::MpscQueue;
use rill_core::NodeId;
use rill_patchbay::{LfoWaveform, PatchbayControl};
use std::sync::Arc;

#[test]
fn test_lfo_automaton_in_control() {
    let queue = Arc::new(MpscQueue::with_capacity(64));
    let mut control = PatchbayControl::new(queue.clone());

    control.add_lfo(
        "test_lfo",
        1.0,
        0.5,
        0.0,
        LfoWaveform::Sine,
        NodeId(1),
        "cutoff",
        100.0,
        1000.0,
    );

    assert!(control.get_servo("test_lfo").is_some());

    for _ in 0..10 {
        control.update(0.1);
    }

    let mut count = 0;
    while queue.pop().is_some() {
        count += 1;
    }
    assert!(count > 0, "Should have sent commands");
}

#[test]
fn test_envelope_in_control() {
    let queue = Arc::new(MpscQueue::with_capacity(64));
    let mut control = PatchbayControl::new(queue.clone());

    control.add_envelope(
        "test_env",
        0.1,
        0.2,
        0.7,
        0.3,
        NodeId(1),
        "gain",
        0.0,
        1.0,
    );

    assert!(control.get_servo("test_env").is_some());
    control.update(0.05);
    control.update(0.05);
}
