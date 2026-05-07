use rill_core::queues::SetParameter;
use rill_core::traits::ActorRef;
use rill_core::NodeId;
use rill_patchbay::{LfoWaveform, PatchbayControl};

#[test]
fn test_lfo_automaton_in_control() {
    let (actor_ref, mailbox) = ActorRef::<SetParameter>::new_pair();
    let mut control = PatchbayControl::new(actor_ref);

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
    while mailbox.pop().is_some() {
        count += 1;
    }
    assert!(count > 0, "Should have sent commands");
}

#[test]
fn test_envelope_in_control() {
    let (actor_ref, _mailbox) = ActorRef::<SetParameter>::new_pair();
    let mut control = PatchbayControl::new(actor_ref);

    control.add_envelope("test_env", 0.1, 0.2, 0.7, 0.3, NodeId(1), "gain", 0.0, 1.0);

    assert!(control.get_servo("test_env").is_some());
    control.update(0.05);
    control.update(0.05);
}
