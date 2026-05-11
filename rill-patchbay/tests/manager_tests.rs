use rill_core::queues::CommandEnum;
use rill_core::NodeId;
use rill_core_actor::{ActorRef, Mbox};
use rill_patchbay::{
    ControlEvent, EventPattern, FunctionAutomaton, LfoWaveform, Mapping, ParameterMapping,
    Patchbay, Servo, Target, Transform,
};
use std::sync::Arc;

fn param(name: &str) -> String {
    name.to_string()
}

#[test]
fn test_control_creation() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    assert_eq!(control.mappings().len(), 0);
    assert!((control.current_time() - 0.0) < 1e-9);
}

#[test]
fn test_add_lfo_servo() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control.add_lfo(
        "lfo1",
        2.0,
        0.3,
        0.5,
        LfoWaveform::Sine,
        NodeId(1),
        "gain",
        0.0,
        1.0,
    );

    assert!(control.get_servo("lfo1").is_some());
}

#[test]
fn test_add_envelope_servo() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control.add_envelope("env1", 0.1, 0.2, 0.7, 0.3, NodeId(1), "gain", 0.0, 1.0);

    assert!(control.get_servo("env1").is_some());
}

#[test]
fn test_add_custom_servo() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    let sine = FunctionAutomaton::new("Sine", |t| (t * 2.0).sin() * 0.5 + 0.5);
    let servo = Servo::new(
        "custom",
        sine,
        NodeId(1),
        param("param"),
        ParameterMapping::Linear,
        0.0,
        1.0,
    );
    control.add_servo(servo);

    assert!(control.get_servo("custom").is_some());
}

#[test]
fn test_remove_servo() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control.add_lfo(
        "lfo1",
        1.0,
        0.2,
        0.5,
        LfoWaveform::Sine,
        NodeId(1),
        "gain",
        0.0,
        1.0,
    );
    control.add_lfo(
        "lfo2",
        2.0,
        0.3,
        0.3,
        LfoWaveform::Sine,
        NodeId(1),
        "pan",
        0.0,
        1.0,
    );

    assert!(control.remove_servo("lfo1"));
    assert!(control.get_servo("lfo1").is_none());
    assert!(control.get_servo("lfo2").is_some());
    assert!(!control.remove_servo("nonexistent"));
}

#[test]
fn test_clear_servos() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control.add_lfo(
        "lfo1",
        1.0,
        0.2,
        0.5,
        LfoWaveform::Sine,
        NodeId(1),
        "gain",
        0.0,
        1.0,
    );
    control.add_lfo(
        "lfo2",
        2.0,
        0.3,
        0.3,
        LfoWaveform::Sine,
        NodeId(1),
        "pan",
        0.0,
        1.0,
    );
    control.add_lfo(
        "lfo3",
        0.5,
        0.1,
        0.7,
        LfoWaveform::Sine,
        NodeId(1),
        "cutoff",
        0.0,
        1.0,
    );

    control.clear();
    assert!(control.get_servo("lfo1").is_none());
    assert!(control.get_servo("lfo2").is_none());
    assert!(control.get_servo("lfo3").is_none());
}

#[test]
fn test_servo_updates() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control.add_lfo(
        "lfo1",
        1.0,
        0.2,
        0.5,
        LfoWaveform::Sine,
        NodeId(1),
        "gain",
        0.0,
        1.0,
    );

    for _ in 1..=3 {
        control.update(0.1);
    }

    let mut count = 0;
    while mailbox.pop().is_some() {
        count += 1;
    }
    assert!(count > 0, "No signals were sent");
}

#[test]
fn test_multiple_servos() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control.add_lfo(
        "lfo1",
        1.0,
        0.2,
        0.5,
        LfoWaveform::Sine,
        NodeId(1),
        "gain",
        0.0,
        1.0,
    );
    control.add_lfo(
        "lfo2",
        2.0,
        0.3,
        0.3,
        LfoWaveform::Sine,
        NodeId(1),
        "pan",
        0.0,
        1.0,
    );
    control.add_lfo(
        "lfo3",
        0.5,
        0.1,
        0.7,
        LfoWaveform::Sine,
        NodeId(1),
        "cutoff",
        0.0,
        1.0,
    );

    assert!(control.get_servo("lfo1").is_some());
    assert!(control.get_servo("lfo2").is_some());
    assert!(control.get_servo("lfo3").is_some());
}

#[test]
fn test_disable_servo() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control.add_lfo(
        "lfo1",
        0.25,
        0.5,
        0.5,
        LfoWaveform::Sine,
        NodeId(1),
        "gain",
        0.0,
        1.0,
    );

    control.update(0.1);
    let initial_count = drain_count(&*mailbox);

    if let Some(servo) = control.get_servo_mut("lfo1") {
        servo.set_enabled(false);
    }

    for _ in 0..3 {
        control.update(0.1);
    }

    let disabled_count = drain_count(&*mailbox);
    assert_eq!(disabled_count, 0, "Signals were sent while disabled");

    if let Some(servo) = control.get_servo_mut("lfo1") {
        servo.set_enabled(true);
    }

    control.update(0.1);
    let after_enable = drain_count(&*mailbox);
    assert!(
        after_enable > 0 || initial_count > 0,
        "Should produce some signals"
    );
}

fn drain_count(mailbox: &Mbox<CommandEnum>) -> usize {
    let mut count = 0;
    while mailbox.pop().is_some() {
        count += 1;
    }
    count
}

#[test]
fn test_different_servo_types() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control.add_lfo(
        "lfo",
        1.0,
        0.5,
        0.5,
        LfoWaveform::Sine,
        NodeId(1),
        "float_param",
        0.0,
        1.0,
    );
    control.add_envelope("env", 0.1, 0.2, 0.7, 0.3, NodeId(1), "int_param", 0.0, 1.0);

    let square =
        FunctionAutomaton::new("Square", |t| if (t * 0.2).sin() > 0.0 { 1.0 } else { 0.0 });
    let servo = Servo::new(
        "gate",
        square,
        NodeId(1),
        "bool_param",
        ParameterMapping::Linear,
        0.0,
        1.0,
    );
    control.add_servo(servo);

    assert!(control.get_servo("lfo").is_some());
    assert!(control.get_servo("env").is_some());
    assert!(control.get_servo("gate").is_some());
}

#[test]
fn test_midi_mapping() {
    let node = NodeId(1);
    let mapping = Mapping::new(
        EventPattern::MidiControl {
            channel: Some(1),
            controller: 7,
        },
        Target {
            node_id: node,
            param_name: "volume".to_string(),
            min: 0.0,
            max: 1.0,
        },
        Transform::Linear,
    );

    let event = ControlEvent::MidiControl {
        channel: 1,
        controller: 7,
        value: 64,
        normalized: 0.5,
    };

    assert!(mapping.matches(&event));

    let cmd = mapping.apply(&event).unwrap();
    assert_eq!(cmd.port.node_id(), node);
    assert_eq!(cmd.parameter.as_ref(), "volume");
    assert!((cmd.value.as_f32().unwrap() - 0.5).abs() < 1e-6);
}

#[test]
fn test_mapping_in_control() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control
        .add_mapping_str("midi:1:7", NodeId(1), "volume", 0.0, 1.0, Transform::Linear)
        .unwrap();

    assert_eq!(control.mappings().len(), 1);

    let event = ControlEvent::MidiControl {
        channel: 1,
        controller: 7,
        value: 64,
        normalized: 0.5,
    };
    control.handle_event(event);

    let cmd = mailbox.pop();
    assert!(cmd.is_some());
    assert!(
        (cmd.unwrap()
            .as_set_parameter()
            .unwrap()
            .value
            .as_f32()
            .unwrap()
            - 0.5)
            .abs()
            < 1e-6
    );
}

#[test]
fn test_reset_time() {
    let mailbox = Arc::new(Mbox::new(64));
    let actor_ref = mailbox.actor_ref();
    let mut control = Patchbay::new(Arc::new(Mbox::new(64)), actor_ref.clone());

    control.update(1.0);
    control.update(2.0);
    assert!((control.current_time() - 3.0).abs() < 1e-9);

    control.reset_time();
    assert!((control.current_time() - 0.0).abs() < 1e-9);
}
