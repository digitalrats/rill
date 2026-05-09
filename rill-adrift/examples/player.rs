//! Load graph from JSON and config from TOML, build and play.
//!
//! Usage:
//!   cargo run --example player --features "cpal,sampler,serialization"
//!   cargo run --example player --features "cpal,sampler,serialization" -- [backend] [wav]
//!   cargo run --example player --features "cpal,sampler,serialization" -- [wav]
//!   cargo run --example player --features "dot,sampler,serialization" -- --dot
//!
//! Positional arguments (optional):
//!   backend   Audio backend name (e.g. cpal, alsa, null). Default from config.toml.
//!   wav       Path to a WAV file to play. Overrides the file in graph.json.
//!
//! --dot: export graph to DOT format and exit

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::registration;
use rill_adrift::rill_core::ParamValue;
use rill_adrift::runtime::{Runtime, RuntimeConfig};
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
    wav_override: Option<&str>,
) -> Result<rill_adrift::rill_graph::Graph<f32, BUF>, Box<dyn std::error::Error>> {
    let graph_path = crate_dir.join(cfg.graph_path.as_deref().unwrap_or("examples/graph.json"));
    let json = std::fs::read_to_string(&graph_path)?;
    let mut def =
        registration::load_graph_json(&json).map_err(|e| format!("load_graph_json: {e}"))?;

    if let Some(wav_path) = wav_override {
        let path = std::path::Path::new(wav_path);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| crate_dir.join(path))
        };
        def.set_node_param(
            0,
            "file",
            ParamValue::String(resolved.to_string_lossy().to_string()),
        );
    }

    let rt = Runtime::<BUF>::new(RuntimeConfig {
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

    let builder = rt
        .create_builder_from_graphdef(&def)
        .map_err(|e| format!("create_builder: {e}"))?;
    let graph = builder.build().map_err(|e| format!("build: {e}"))?;
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

    if args.iter().any(|a| a == "--dot") {
        #[cfg(feature = "dot")]
        {
            let graph = build_graph(&cfg, &crate_dir, "null", None)?;
            let dot = rill_adrift::rill_graph::dot::to_dot(
                &graph,
                &rill_adrift::rill_graph::dot::DotConfig::default(),
            );
            println!("{dot}");
        }
        #[cfg(not(feature = "dot"))]
        eprintln!("Enable --features dot for DOT export.");
        return Ok(());
    }

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
        let wav_file = wav_arg.map(|s| s.to_string());
        std::thread::spawn(move || {
            let mut graph = build_graph(&cfg, &crate_dir, &backend_name, wav_file.as_deref())
                .expect("build_graph");
            graph.run(running).ok();
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
