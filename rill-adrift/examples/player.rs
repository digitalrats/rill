//! Load config from TOML, compile a signal graph via rill-lang DSL, and play a WAV.
//!
//! Usage:
//!   cargo run --example player --features "io,lang,sampler,serialization" [backend] [wav]
//!   cargo run --example player --features "io,lang,sampler,serialization" -- [wav]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::backend_factory::{BackendFactory, OutputBundle};
use rill_adrift::registration;
use rill_adrift::rill_core::{
    queues::{CommandEnum, SetParameter, SignalOrigin},
    traits::{ParamValue, ParameterId, SignalSlab},
};
use rill_lang::program_runner::ProgramRunner;
use rill_lang::runtime::Runtime;
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

    let audio_backend = backend_name.clone();
    let wav_path = wav_arg.map(ToString::to_string);

    let t_run = running.clone();

    let signal_thread = std::thread::spawn(move || {
        let mut bf = BackendFactory::new();
        registration::register_backends(&mut bf);
        let mut be_params = HashMap::new();
        be_params.insert("sample_rate".into(), ParamValue::Float(cfg.sample_rate));
        be_params.insert("buffer_size".into(), ParamValue::Int(cfg.block_size as i32));
        be_params.insert("channels".into(), ParamValue::Int(2));
        let OutputBundle { driver, playback } = bf
            .create_output(&audio_backend, &be_params)
            .expect("create output backend");

        let slab: Option<Arc<SignalSlab>> =
            wav_path
                .as_ref()
                .and_then(|path| match rill_adrift::sampler::wav::load_slab(path) {
                    Ok(s) => {
                        eprintln!("SamplePlayer: loaded {path}");
                        Some(Arc::new(s))
                    }
                    Err(e) => {
                        eprintln!("SamplePlayer: could not load {path}: {e}");
                        None
                    }
                });

        let reg = rill_adrift::lang_builtins::full_registry_f32();
        // `s` is a closed top-level def (0 input channels) — a CAF — so the
        // broadcast `s , s` shares ONE sampler instance: both stereo outputs
        // carry a copy of the same mono sample.
        let src = "s = sampler 1.0 1.0 1.0 0.0 ?source; main = s , s";
        let engine =
            rill_lang::compile_graph::<f32, 256>(src, &reg, cfg.sample_rate).expect("compile DSL");
        let runner = ProgramRunner::new(engine, None);
        let handle = runner.handle();

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

        Runtime::launch::<256>(driver, None, Some(playback), runner, t_run).ok();
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

    println!("Playing WAV through {backend_name} backend. Press Enter to stop.");
    signal_input.join().ok();
    signal_thread.join().ok();
    println!("Stopped.");
    Ok(())
}
