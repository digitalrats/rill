//! Load a graph from a JSON file and play it through the selected backend.
//!
//! Usage:
//!   cargo run --example play_json --features "cpal,sampler,serialization" -- [backend] [wav]
//!   cargo run --example play_json --features "dot,sampler,serialization" -- --dot
//!
//! Backend: "cpal" (default), "alsa", "pipewire", "jack", "null"
//! WAV:     override the sample file (sent through command queue)
//! --dot:   export graph to DOT format and exit

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::registration;
use rill_adrift::rill_core::queues::{SetParameter, SignalOrigin};
use rill_adrift::rill_core::time::SystemClock;
use rill_adrift::rill_core::traits::{NodeId, ParamValue, ParameterId, PortId};
use rill_adrift::rill_graph::backend_factory::{BackendConfig, BackendFactory};

const BUF: usize = 256;
const RATE: f32 = 44100.0;

type Graph = rill_adrift::rill_graph::Graph<f32, BUF>;

fn build_graph(json: &str, backend_name: &str) -> Graph {
    let builder = registration::load_graph_json::<BUF>(json).expect("load_graph_json");

    let mut backend_factory = BackendFactory::<f32>::new();
    registration::register_backends(&mut backend_factory);

    let clock = Box::new(SystemClock::with_sample_rate(RATE));
    builder
        .build(
            clock,
            Some(&BackendConfig {
                factory: &backend_factory,
                name: backend_name,
                sample_rate: RATE as u32,
                buffer_size: BUF as u32,
                channels: 2,
            }),
        )
        .expect("graph build")
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let graph_path = crate_dir.join("examples/graph.json");
    let json = std::fs::read_to_string(&graph_path)
        .map_err(|e| format!("Cannot read {}: {e}", graph_path.display()))?;

    // --dot: export graph to DOT and exit
    if args.iter().any(|a| a == "--dot") {
        #[cfg(feature = "dot")]
        {
            let graph = build_graph(&json, "null");
            let dot = rill_adrift::rill_graph::dot::to_dot(
                &graph,
                &rill_adrift::rill_graph::dot::DotConfig::default(),
            );
            println!("{}", dot);
        }
        #[cfg(not(feature = "dot"))]
        eprintln!("Enable --features dot for DOT export.");
        return Ok(());
    }

    let backend_name_arg = args.get(1).cloned().unwrap_or_else(|| "null".into());
    let wav_file = args.get(2).cloned();
    let backend_name = backend_name_arg.clone();

    let running = Arc::new(AtomicBool::new(true));

    // Build graph and run it on a dedicated audio thread.
    let t_run = running.clone();
    let audio_thread = std::thread::spawn(move || {
        let graph = build_graph(&json, &backend_name);
        let cmd_queue = graph.handle();

        // Override sample file via command queue (before run, applies on first callback)
        if let (Some(ref path), Some(ref q)) = (wav_file.as_ref(), cmd_queue.as_ref()) {
            q.send(SetParameter::new(
                PortId::param(NodeId(0), 0),
                ParameterId::new("file").unwrap(),
                ParamValue::String(path.to_string()),
                SignalOrigin::Manual,
            ));
        }

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
        "▶ Playing graph from {} through {} backend. Press Enter to stop.",
        graph_path.display(),
        backend_name_arg,
    );

    signal_thread.join().ok();
    audio_thread.join().ok();

    println!("⏹ Stopped.");
    Ok(())
}
