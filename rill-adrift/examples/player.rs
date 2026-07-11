//! Load config from TOML, compile a signal graph via rill-lang DSL, and play.
//!
//! Usage:
//!   cargo run --example player --features "io,lang,sampler,serialization" [backend] [wav]
//!   cargo run --example player --features "io,lang,sampler,serialization" -- [wav]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::registration;
use rill_adrift::rill_core::traits::ParamValue;
use rill_adrift::rill_graph::backend_factory::{BackendFactory, OutputBundle};
use rill_lang::program_runner::ProgramRunner;
use serde::Deserialize;

#[derive(Deserialize, Clone)]
struct BackendCfg {
    name: String,
}

#[derive(Deserialize, Clone)]
struct AppConfig {
    sample_rate: f32,
    block_size: usize,
    backend: Option<BackendCfg>,
}

fn load_config() -> Result<AppConfig, Box<dyn std::error::Error>> {
    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let config_path = crate_dir.join("examples/config.toml");
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Cannot read {}: {e}", config_path.display()))?;
    let cfg: AppConfig = toml::from_str(&content)?;
    Ok(cfg)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = load_config()?;
    let args: Vec<String> = std::env::args().collect();
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();

    let (backend_arg, _wav_arg): (Option<&str>, Option<&str>) = match positional.len() {
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

    let audio_backend = backend_name.clone();
    let t_run = running.clone();

    let signal_thread = std::thread::spawn(move || {
        // Create I/O backend
        let mut bf = BackendFactory::new();
        registration::register_backends(&mut bf);
        let mut be_params = HashMap::new();
        be_params.insert("sample_rate".into(), ParamValue::Float(cfg.sample_rate));
        be_params.insert("buffer_size".into(), ParamValue::Int(cfg.block_size as i32));
        be_params.insert("channels".into(), ParamValue::Int(2));
        let OutputBundle { driver, playback } = bf
            .create_output(&audio_backend, &be_params)
            .expect("create output backend");

        // Compile rill-lang DSL: pass-through identity.
        let reg = rill_adrift::lang_builtins::full_registry::<f32>();
        let src = "main = _";
        let engine =
            rill_lang::compile_graph::<f32>(src, &reg, cfg.sample_rate).expect("compile DSL");
        let mut runner = ProgramRunner::new(engine, None, cfg.block_size);
        runner.wire_backends(None, Some(playback));
        runner.run_with_driver(driver, t_run).ok();
    });

    let signal_input = std::thread::spawn({
        let running = running.clone();
        let signal_handle = signal_thread.thread().clone();
        move || {
            let mut input = String::new();
            let _ = std::io::stdin().read_line(&mut input);
            running.store(false, Ordering::Release);
            signal_handle.unpark();
        }
    });

    println!("Playing graph through {backend_name} backend. Press Enter to stop.");
    signal_input.join().ok();
    signal_thread.join().ok();
    println!("Stopped.");
    Ok(())
}
