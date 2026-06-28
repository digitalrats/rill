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
use rill_adrift::rill_core::{
    queues::{CommandEnum, SetParameter, SignalOrigin},
    traits::{Params, SignalSlab},
    NodeId, ParamValue, ParameterId, PortId,
};
use rill_adrift::rill_core_actor::ActorSystem;
use rill_adrift::rill_graph::backend_factory::{BackendFactory, OutputBundle};
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
        // ── 1. Register backends first ─────────────────
        let mut bf = BackendFactory::new();
        registration::register_backends(&mut bf);
        let mut be_params = HashMap::new();
        be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
        be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
        be_params.insert("channels".into(), ParamValue::Int(2));
        let OutputBundle { driver, playback } = bf
            .create_output(&be_name, &be_params)
            .expect("create output backend");

        // ── Load WAV on control thread ────
        let slab: Option<Arc<SignalSlab>> = match rill_adrift::sampler::wav::load_slab(&wav_path) {
            Ok(s) => {
                eprintln!("Loaded {wav_path}");
                Some(Arc::new(s))
            }
            Err(e) => {
                eprintln!("Could not load {wav_path}: {e}");
                None
            }
        };

        // ── 2. Register node types ──────────────────
        let mut factory = NodeFactory::<f32, BUF>::new();
        registration::register_all_nodes::<BUF>(&mut factory);

        // ── 3. Build graph ──────────────────────────────────────────
        let mut builder = GraphBuilder::new(Arc::new(factory));

        let sampler = builder.add_node("rill/sampler", &Params::new(RATE));
        let output = builder.add_node("rill/output", &Params::new(RATE));
        builder.connect_signal(sampler, 0, output, 0);
        builder.connect_signal(sampler, 1, output, 1);

        // ── Build and run ────────────────────────────────────────
        let system = ActorSystem::new();
        match builder.build(&system) {
            Ok(graph) => {
                // Send slab via actor mailbox for zero-alloc RT swap
                let handle = graph.handle();
                if let Some(ref s) = slab {
                    handle.send(CommandEnum::SetParameter(SetParameter::new(
                        PortId::signal_out(NodeId(0), 0),
                        ParameterId::new("source").unwrap(),
                        ParamValue::SignalSlab(s.clone()),
                        SignalOrigin::Manual,
                    )));
                }
                drop(slab);

                eprintln!("Graph built ({} nodes). Playing...", graph.node_count());
                let mut state = graph.into_processing_state();
                state.wire_backends(None, Some(playback));
                if let Err(e) = state.run_with_driver(driver, t_run) {
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
