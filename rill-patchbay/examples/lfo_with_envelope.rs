use rill_core::NodeId;
use rill_core_actor::ActorRef;
use rill_patchbay::{LfoWaveform, Patchbay};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== LFO + Envelope Example ===\n");

    let (actor_ref, mailbox) = ActorRef::new_pair();
    let mut control = Patchbay::new(actor_ref);
    let node = NodeId(1);

    // LFO
    control.add_lfo(
        "lfo",
        0.5,
        0.8,
        0.5,
        LfoWaveform::Sine,
        node,
        "modulator",
        0.0,
        1.0,
    );

    // Envelope
    control.add_envelope("env", 0.1, 0.2, 0.7, 0.3, node, "amplifier", 0.0, 1.0);

    println!("Components added. Running updates...\n");
    println!("Time(s)\tCommands in queue");
    println!("-------\t-----------------");

    for i in 0..20 {
        let time = i as f64 * 0.1;
        control.update(0.1);
        let count = std::iter::from_fn(|| mailbox.pop()).count();
        println!("{:.1}\t{}", time, count);
    }

    println!("\nDone");
    Ok(())
}
