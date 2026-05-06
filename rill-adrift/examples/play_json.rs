//! Load a graph from a JSON file and play it through the selected backend.
//!
//! Usage:
//!   cargo run --example play_json -- [backend] [graph.json]
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
    let backend_name = args.get(1).map(|s| s.as_str()).unwrap_or("cpal");
    let graph_path = args
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or("examples/graph.json");

    // Load graph from JSON
    let json = std::fs::read_to_string(graph_path)
        .map_err(|e| format!("Cannot read {graph_path}: {e}"))?;
    let builder = registration::load_graph_json::<BUF>(&json)?;

    // Create backend factory
    let mut backend_factory = BackendFactory::<f32>::new();
    registration::register_backends(&mut backend_factory);

    let backend_cfg = BackendConfig {
        factory: &backend_factory,
        name: backend_name,
        sample_rate: RATE as u32,
        buffer_size: BUF as u32,
        channels: 2,
    };

    let clock = Box::new(SystemClock::with_sample_rate(RATE));
    let mut graph = builder
        .build(clock, Some(&backend_cfg))
        .expect("graph build");

    let running = Arc::new(AtomicBool::new(true));
    let t_running = running.clone();

    // Get a static reference to the backend (graph lives until end of main)
    let backend_static: &'static dyn rill_adrift::rill_core::io::IoBackend<f32> =
        unsafe { std::mem::transmute(graph.backend_ref().unwrap()) };

    let audio_thread = std::thread::spawn(move || {
        let _ = backend_static.run(t_running.clone());
        while t_running.load(Ordering::Acquire) {
            std::thread::park();
        }
        let _ = backend_static.stop();
    });

    println!(
        "▶ Playing graph from {graph_path} through {backend_name} backend. Press Enter to stop."
    );
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    running.store(false, Ordering::Release);
    audio_thread.thread().unpark();
    let _ = audio_thread.join();

    println!("⏹ Stopped.");
    Ok(())
}
