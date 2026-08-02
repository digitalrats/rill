//! Load graph from JSON and config from TOML, build and play.
//!
//! Demonstrates runtime parameter control via the actor mailbox:
//! the graph is built from `graph.json` via the serialisation layer,
//! then parameter changes (WAV slab for the sampler, cutoff for the
//! biquad filter) are sent through the actor mailbox before the
//! signal thread starts.
//!
//! Usage:
//!   cargo run --example advanced_player --features "io,portaudio,sampler,serialization"
//!   cargo run --example advanced_player --features "io,portaudio,sampler,serialization" -- [backend] [wav]
//!   cargo run --example advanced_player --features "io,portaudio,sampler,serialization" -- [wav]
//!
//! Positional arguments (optional):
//!   backend   I/O backend name (e.g. portaudio, alsa, null). Default from config.toml.
//!   wav       Path to a WAV file to play. Sent as a `SetParameter` command
//!             via the graph's actor mailbox.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::modular::{ModularConfig, ModularSystem};
use rill_adrift::registration;
use rill_adrift::rill_core::{
    queues::{CommandEnum, SetParameter, SignalOrigin},
    traits::{ParamValue, ParameterId, SignalSlab},
};
use rill_adrift::rill_graph::backend_factory::{BackendFactory, OutputBundle};
use rill_lang::program_runner::ProgramRunner;
use rill_lang::runtime::Runtime;
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

fn resolve_wav_path(wav_path: &str, crate_dir: &std::path::Path) -> String {
    let path = std::path::Path::new(wav_path);
    if path.is_absolute() {
        path.to_string_lossy().to_string()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| crate_dir.join(path))
            .to_string_lossy()
            .to_string()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_config()?;
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

    let args: Vec<String> = std::env::args().collect();

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

    let wav_path = wav_arg.map(|s| resolve_wav_path(s, crate_dir));

    let running = Arc::new(AtomicBool::new(true));

    let signal_thread = {
        let cfg = cfg.clone();
        let running = running.clone();
        let crate_dir = crate_dir.to_path_buf();
        let backend_name = backend_name.clone();
        let wav_path = wav_path.clone();
        std::thread::spawn(move || {
            let mut bf = BackendFactory::new();
            registration::register_backends(&mut bf);
            let mut be_params = HashMap::new();
            be_params.insert("sample_rate".into(), ParamValue::Float(cfg.sample_rate));
            be_params.insert("buffer_size".into(), ParamValue::Int(cfg.block_size as i32));
            be_params.insert("channels".into(), ParamValue::Int(2));
            let OutputBundle { driver, playback } = bf
                .create_output(&backend_name, &be_params)
                .expect("create output backend");

            // Load WAV file on control thread BEFORE graph processing starts.
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

            let graph_path =
                crate_dir.join(cfg.graph_path.as_deref().unwrap_or("examples/graph.json"));
            let json = std::fs::read_to_string(&graph_path).expect("read graph.json");
            let graph_def = registration::load_graph_json(&json).expect("load_graph_json");

            let system = ModularSystem::<BUF>::new(ModularConfig {
                sample_rate: cfg.sample_rate,
                block_size: cfg.block_size,
                backend_name: Some(backend_name.clone()),
                backend_params: cfg
                    .backend
                    .as_ref()
                    .map(|b| b.params.clone())
                    .unwrap_or_default(),
                ..Default::default()
            });

            let engine = system.build_engine(&graph_def).expect("build_engine");

            let runner = ProgramRunner::new(engine, None);

            let handle = runner.handle();
            // Send WAV slab to the sampler via SetParameter.
            if let Some(ref s) = slab {
                let sp = SetParameter::new(
                    "".into(),
                    ParameterId::new("source").unwrap(),
                    ParamValue::SignalSlab(s.clone()),
                    SignalOrigin::Manual,
                );
                handle.send(CommandEnum::SetParameter(sp));
            }
            drop(slab);

            // Set biquad filter cutoff to 800 Hz.
            let sp = SetParameter::new(
                "".into(),
                ParameterId::new("cutoff").unwrap(),
                ParamValue::Float(800.0),
                SignalOrigin::Manual,
            );
            handle.send(CommandEnum::SetParameter(sp));

            if let Err(e) = Runtime::launch::<BUF>(driver, None, Some(playback), runner, running) {
                eprintln!("Backend error: {e}");
            }
        })
    };

    let signalled = {
        let running = running.clone();
        let signal_handle = signal_thread.thread().clone();
        std::thread::spawn(move || {
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            running.store(false, Ordering::Release);
            signal_handle.unpark();
        })
    };

    println!("\u{25B6} Playing graph from graph.json through {backend_name} backend. Press Enter to stop.");
    signalled.join().ok();
    signal_thread.join().ok();
    println!("\u{23F9} Stopped.");
    Ok(())
}
