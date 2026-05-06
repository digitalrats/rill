//! Load a graph from a JSON file and play it through the selected backend.
//!
//! Usage:
//!   cargo run --example play_json --features "cpal,sampler,serialization" -- [backend] [graph.json]
//!
//! Backend: "cpal" (default), "alsa", "pipewire", "jack", "null"
//! Graph:   path to JSON file (default: examples/graph.json)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::registration;
use rill_adrift::rill_core::time::SystemClock;
use rill_adrift::rill_graph::backend_factory::{BackendConfig, BackendFactory};

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args.get(1).cloned().unwrap_or_else(|| "cpal".into());
    let backend_name_clone = backend_name.clone();
    let graph_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "examples/graph.json".into());

    let json = std::fs::read_to_string(&graph_path)
        .map_err(|e| format!("Cannot read {graph_path}: {e}"))?;

    let running = Arc::new(AtomicBool::new(true));

    // Build graph and run it on a dedicated audio thread.
    // The graph owns the backend — no external access.
    let t_run = running.clone();
    let audio_thread = std::thread::spawn(move || {
        let builder = registration::load_graph_json::<BUF>(&json).expect("load_graph_json");

        let mut backend_factory = BackendFactory::<f32>::new();
        registration::register_backends(&mut backend_factory);

        let clock = Box::new(SystemClock::with_sample_rate(RATE));
        let graph = builder
            .build(
                clock,
                Some(&BackendConfig {
                    factory: &backend_factory,
                    name: &backend_name_clone,
                    sample_rate: RATE as u32,
                    buffer_size: BUF as u32,
                    channels: 2,
                }),
            )
            .expect("graph build");

        // graph.run() parks for non-blocking backends;
        // signal thread unparks after Enter.
        graph.run(t_run).ok();
    });

    // Wait for Enter, then signal the audio thread to stop
    let t_run = running.clone();
    let audio_handle = audio_thread.thread().clone();
    let signal_thread = std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        t_run.store(false, Ordering::Release);
        audio_handle.unpark();
    });

    println!(
        "▶ Playing graph from {graph_path} through {} backend. Press Enter to stop.",
        backend_name
    );

    signal_thread.join().ok();
    audio_thread.join().ok();

    println!("⏹ Stopped.");
    Ok(())
}
