//! Load graph from JSON and config from TOML, build and play.
//!
//! Usage:
//!   cargo run --example player --features "portaudio,sampler,serialization"
//!   cargo run --example player --features "portaudio,sampler,serialization" -- [backend] [wav]
//!   cargo run --example player --features "portaudio,sampler,serialization" -- [wav]
//! Positional arguments (optional):
//!   backend   I/O backend name (e.g. portaudio, alsa, null). Default from config.toml.
//!   wav       Path to a WAV file to play. Overrides the file in graph.json.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::modular::{ModularConfig, ModularSystem};
use rill_adrift::registration;
use rill_adrift::rill_core::{
    queues::{CommandEnum, SetParameter, SignalOrigin},
    traits::SignalSlab,
    NodeId, ParamValue, ParameterId, PortId,
};
use rill_adrift::rill_graph::backend_factory::{BackendFactory, OutputBundle};
use serde::Deserialize;

const BUF: usize = 256;

#[derive(Deserialize, Clone)]
struct BackendCfg {
    name: String,
    #[serde(default)]
    params: HashMap<String, String>,
}

#[derive(Deserialize, Clone)]
struct AppConfig {
    sample_rate: f32,
    block_size: usize,
    backend: Option<BackendCfg>,
    #[serde(default)]
    graph_path: Option<String>,
}

fn load_config() -> Result<AppConfig, Box<dyn std::error::Error>> {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let config_path = crate_dir.join("examples/config.toml");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Cannot read {}: {e}", config_path.display()))?;
    let cfg: AppConfig = toml::from_str(&content)?;
    Ok(cfg)
}

fn build_graph(
    cfg: &AppConfig,
    crate_dir: &std::path::Path,
    backend_name: &str,
) -> Result<rill_adrift::rill_graph::Graph<f32, BUF>, Box<dyn std::error::Error>> {
    let graph_path = crate_dir.join(cfg.graph_path.as_deref().unwrap_or("examples/graph.json"));
    let json = std::fs::read_to_string(&graph_path)?;
    let def =
        registration::load_graph_json(&json).map_err(|e| format!("load_graph_json: {e}"))?;

    let system = ModularSystem::<BUF>::new(ModularConfig {
        sample_rate: cfg.sample_rate,
        block_size: cfg.block_size,
        backend_name: Some(backend_name.to_string()),
        backend_params: cfg
            .backend
            .as_ref()
            .map(|b| b.params.clone())
            .unwrap_or_default(),
        ..Default::default()
    });

    let graph = system
        .build_graph(&def)
        .map_err(|e| format!("build: {e}"))?;
    Ok(graph)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_config()?;
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    let args: Vec<String> = std::env::args().collect();

    // Parse optional positional arguments:
    //   positional[0] = backend name OR wav file
    //   positional[1] = wav file (when positional[0] is a backend name)
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();

    let (backend_arg, wav_arg): (Option<&str>, Option<&str>) = match positional.len() {
        0 => (None, None),
        1 => {
            let v = positional[0].as_str();
            if v.ends_with(".wav") || std::path::Path::new(v).is_file() {
                (None, Some(v))
            } else {
                (Some(v), None)
            }
        }
        _ => (Some(positional[0].as_str()), Some(positional[1].as_str())),
    };
    let backend_name = backend_arg
        .map(|s| s.to_string())
        .or_else(|| cfg.backend.as_ref().map(|b| b.name.clone()))
        .unwrap_or_else(|| "null".into());
    let running = Arc::new(AtomicBool::new(true));

    let audio_thread = {
        let cfg = cfg.clone();
        let running = running.clone();
        let crate_dir = crate_dir.to_path_buf();
        let backend_name = backend_name.clone();
        let wav_path = wav_arg
            .map(|s| {
                let p = std::path::Path::new(s);
                if p.is_absolute() {
                    p.to_string_lossy().to_string()
                } else {
                    std::env::current_dir()
                        .map(|cwd| cwd.join(p).to_string_lossy().to_string())
                        .unwrap_or_else(|_| crate_dir.join(p).to_string_lossy().to_string())
                }
            });
        std::thread::spawn(move || {
            // ── 1. Create backend before graph construction ──
            let mut bf = BackendFactory::new();
            registration::register_backends(&mut bf);
            let mut be_params = HashMap::new();
            be_params.insert("sample_rate".into(), ParamValue::Float(cfg.sample_rate));
            be_params.insert("buffer_size".into(), ParamValue::Int(cfg.block_size as i32));
            be_params.insert("channels".into(), ParamValue::Int(2));
            let OutputBundle { driver, playback } =
                bf.create_output(&backend_name, &be_params).expect("create output backend");

            // Load WAV on control thread
            let slab: Option<Arc<SignalSlab>> = wav_path.as_ref().and_then(|path| {
                match rill_adrift::sampler::wav::load_slab(path) {
                    Ok(s) => {
                        eprintln!("SamplePlayer: loaded {path}");
                        Some(Arc::new(s))
                    }
                    Err(e) => {
                        eprintln!("SamplePlayer: could not load {path}: {e}");
                        None
                    }
                }
            });

            let graph = build_graph(&cfg, &crate_dir, &backend_name)
                .expect("build_graph");

            // Send slab via actor mailbox
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

            let mut state = graph.into_processing_state();
            state.wire_backends(None, Some(playback));
            if let Err(e) = state.run_with_driver(driver, running) {
                eprintln!("Backend error: {e}");
            }
        })
    };

    let signal_thread = {
        let running = running.clone();
        let audio_handle = audio_thread.thread().clone();
        std::thread::spawn(move || {
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            running.store(false, Ordering::Release);
            audio_handle.unpark();
        })
    };

    println!("▶ Playing graph through {backend_name} backend. Press Enter to stop.");
    signal_thread.join().ok();
    audio_thread.join().ok();
    println!("⏹ Stopped.");
    Ok(())
}
