use rill_core::NodeId;
use rill_core_actor::ActorRef;
use rill_patchbay::{FunctionAutomaton, LfoWaveform, ParameterMapping, Patchbay, Servo};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Multiple Servos Example ===\n");

    let (actor_ref, mailbox) = ActorRef::new_pair();
    let mut control = Patchbay::new(actor_ref);
    let node = NodeId(1);

    // Three different automata
    control.add_lfo(
        "lfo_sine",
        1.0,
        0.3,
        0.5,
        LfoWaveform::Sine,
        node,
        "gain",
        0.0,
        1.0,
    );
    control.add_lfo(
        "lfo_tri",
        0.5,
        0.4,
        0.3,
        LfoWaveform::Triangle,
        node,
        "pan",
        -1.0,
        1.0,
    );

    let square =
        FunctionAutomaton::new("Square", |t| if (t * 2.0).sin() > 0.0 { 1.0 } else { 0.0 });
    let servo = Servo::new(
        "gate",
        square,
        node,
        "mute",
        ParameterMapping::Linear,
        0.0,
        1.0,
    );
    control.add_servo(servo);

    control.update(0.0);

    println!("Active servos (implicitly via Patchbay internals)\n");
    println!("Running 10 updates at 10ms each...\n");

    for i in 0..10 {
        control.update(0.01);
        let cmds: Vec<_> = std::iter::from_fn(|| mailbox.pop()).collect();
        println!("Update {}: {} command(s) sent", i + 1, cmds.len());
    }

    println!("\nDone");
    Ok(())
}
