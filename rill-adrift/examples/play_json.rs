//! Load a graph from a JSON file and play it through the selected backend.
//!
//! Usage:
//!   cargo run --example play_json --features "cpal,sampler,serialization" -- [backend] [graph.json]
//!
//! Backend: "cpal" (default), "alsa", "pipewire", "jack", "null"
//! Graph:   path to JSON file (default: examples/graph.json)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name_arg = args.get(1).cloned().unwrap_or_else(|| "cpal".into());
    let graph_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "examples/graph.json".into());
    let backend_name = backend_name_arg.clone();

    let json = std::fs::read_to_string(&graph_path)
        .map_err(|e| format!("Cannot read {graph_path}: {e}"))?;

    let running = Arc::new(AtomicBool::new(true));

    // Wait for Enter on a dedicated thread
    let t_run = running.clone();
    let input_thread = std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        t_run.store(false, Ordering::Release);
    });

    // Build graph, run backend — all on the audio thread
    let t_run = running.clone();
    let audio_thread = std::thread::spawn(move || {
        let builder =
            rill_adrift::registration::load_graph_json::<BUF>(&json).expect("load_graph_json");

        let mut backend_factory = rill_adrift::rill_graph::backend_factory::BackendFactory::new();
        rill_adrift::registration::register_backends(&mut backend_factory);

        let backend_cfg = rill_adrift::rill_graph::backend_factory::BackendConfig {
            factory: &backend_factory,
            name: &backend_name,
            sample_rate: RATE as u32,
            buffer_size: BUF as u32,
            channels: 2,
        };

        let clock = Box::new(rill_adrift::rill_core::time::SystemClock::with_sample_rate(
            RATE,
        ));
        let graph = builder
            .build(clock, Some(&backend_cfg))
            .expect("graph build");

        let backend = graph.backend_ref().expect("backend exists");
        let _ = backend.run(t_run.clone());
        while t_run.load(Ordering::Acquire) {
            std::thread::park();
        }
        let _ = backend.stop();
    });

    println!(
        "▶ Playing graph from {graph_path} through {backend_name_arg} backend. Press Enter to stop."
    );

    let _ = input_thread.join();
    running.store(false, Ordering::Release);
    audio_thread.thread().unpark();
    let _ = audio_thread.join();

    println!("⏹ Stopped.");
    Ok(())
}
