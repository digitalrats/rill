use rill_core::queues::MpscQueue;
use rill_core::NodeId;
use rill_patchbay::{LfoWaveform, PatchbayControl};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Basic LFO Automation Example ===\n");

    let queue = Arc::new(MpscQueue::with_capacity(1024));
    let mut control = PatchbayControl::new(queue.clone());
    let node = NodeId(1);

    control.add_lfo(
        "volume_lfo",
        0.5,
        0.3,
        0.5,
        LfoWaveform::Sine,
        node,
        "volume",
        0.0,
        1.0,
    );

    println!("LFO added. Starting automation...\n");
    println!("Time(s)\tValue");
    println!("-------\t-----");

    for i in 0..20 {
        let time = i as f64 * 0.5;
        control.update(0.5);

        let value = queue
            .pop()
            .map(|cmd| cmd.value)
            .unwrap_or(rill_core::traits::ParamValue::Float(0.5));
        println!("{:.1}\t{:.3}", time, value.as_f32().unwrap_or(0.0));
    }

    println!("\nDone");
    Ok(())
}
