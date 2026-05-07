use rill_core::NodeId;
use rill_core_actor::ActorRef;
use rill_patchbay::{
    FunctionAutomaton, ParameterMapping, Patchbay, Servo, StatefulFunctionAutomaton,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Custom Automaton Examples ===\n");

    let (actor_ref, _mailbox) = ActorRef::new_pair();
    let mut control = Patchbay::new(actor_ref);
    let node = NodeId(1);

    // Example 1: Simple closure
    println!("1. Simple sine:");
    let sine = FunctionAutomaton::new("Sine", |t| (t * 0.5).sin() * 0.3 + 0.5);
    let servo = Servo::new(
        "simple",
        sine,
        node,
        "volume",
        ParameterMapping::Linear,
        0.0,
        1.0,
    );
    control.add_servo(servo);

    // Example 2: Stateful function (counter)
    println!("\n2. Stateful counter:");
    let counter = StatefulFunctionAutomaton::new(
        "Counter",
        |_t, count: &mut f64| {
            *count += 0.01;
            if *count > 1.0 {
                *count = 0.0;
            }
            *count
        },
        0.0,
    );
    let servo = Servo::new(
        "counter",
        counter,
        node,
        "position",
        ParameterMapping::Linear,
        0.0,
        1.0,
    );
    control.add_servo(servo);

    println!("\nUpdating for 2 seconds...\n");
    for i in 0..20 {
        control.update(0.1);
        println!("t={:.1}s  (updated)", i as f64 * 0.1);
    }

    println!("\nDone");
    Ok(())
}
