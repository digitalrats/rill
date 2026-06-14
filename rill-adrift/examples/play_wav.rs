//! Play a WAV file — simplest possible rill graph example.
//!
//! Builds the graph manually: sampler source → signal output sink.
//! No config files, no ModularSystemDef — just a hand-built graph.
//!
//! Usage:
//!   cargo run --example play_wav --features "io,sampler,portaudio"
//!   cargo run --example play_wav --features "io,sampler,portaudio" -- [backend] [wav_path]
//!   cargo run --example play_wav --features "io,sampler,alsa" -- alsa myfile.wav

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::registration;
use rill_adrift::rill_core::traits::{ParamValue, Params};
use rill_adrift::rill_core_actor::ActorSystem;
use rill_adrift::rill_graph::backend_factory::BackendFactory;
use rill_adrift::rill_graph::{GraphBuilder, NodeFactory};

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let args: Vec<String> = std::env::args().collect();
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();

    let default_wav = crate_dir
        .join("ESW Aura Inst - LoFi Steel - C.wav")
        .to_string_lossy()
        .to_string();

    let (backend_name, wav_path): (String, String) = match positional.len() {
        0 => ("portaudio".into(), default_wav),
        1 => {
            let v = positional[0].as_str();
            if v.ends_with(".wav") || std::path::Path::new(v).is_file() {
                ("portaudio".into(), v.to_string())
            } else {
                (v.to_string(), default_wav)
            }
        }
        _ => (positional[0].clone(), positional[1].clone()),
    };

    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();
    let wav_display = wav_path.clone();
    let be_name = backend_name.clone();

    let audio_thread = std::thread::spawn(move || {
        // ── Register node types ──────────────────
        let mut factory = NodeFactory::<f32, BUF>::new();
        registration::register_all_nodes::<BUF>(&mut factory);

        // ── Register backends ────────────────────
        let mut bf = BackendFactory::new();
        registration::register_backends(&mut bf);

        // ── Build graph ──────────────────────────────────────────
        let mut builder = GraphBuilder::new(Arc::new(factory));

        let mut sampler_params = Params::new(RATE);
        sampler_params.insert("file", ParamValue::String(wav_path.clone()));

        let sampler = builder.add_node("rill/sampler", &sampler_params);
        let output = builder.add_node("rill/output", &Params::new(RATE));
        builder.connect_signal(sampler, 0, output, 0);
        builder.connect_signal(sampler, 1, output, 1);

        // ── Build and run ────────────────────────────────────────
        let system = ActorSystem::new();
        match builder.build(&system) {
            Ok(graph) => {
                eprintln!("Graph built ({} nodes). Playing...", graph.node_count());
                let mut state = graph.into_processing_state();
                let mut be_params = HashMap::new();
                be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
                be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
                be_params.insert("channels".into(), ParamValue::Int(2));
                if let Err(e) = state.run_with_backend(&bf, &be_name, &be_params, t_run) {
                    eprintln!("Backend error: {e}");
                }
            }
            Err(e) => eprintln!("Build error: {e:?}"),
        }
    });

    let r = running.clone();
    let handle = audio_thread.thread().clone();
    let signal_thread = std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        r.store(false, Ordering::Release);
        handle.unpark();
    });

    println!(
        "▶ Playing {} through {} backend. Press Enter to stop.",
        wav_display, backend_name
    );

    signal_thread.join().ok();
    audio_thread.join().ok();
    println!("⏹ Stopped.");
    Ok(())
}
