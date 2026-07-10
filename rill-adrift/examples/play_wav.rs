//! Play a WAV file — simplest possible rill-lang DSL example.
//!
//! Uses rill-lang DSL to compile a simple sampler program. WAV data is
//! loaded before compilation and embedded directly in the graph.
//!
//! Usage:
//!   cargo run --example play_wav --features "io,lang,sampler,portaudio"
//!   cargo run --example play_wav --features "io,lang,sampler,portaudio" -- [backend] [wav_path]
//!   cargo run --example play_wav --features "io,lang,sampler,alsa" -- alsa myfile.wav

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::registration;
use rill_adrift::rill_core::traits::ParamValue;
use rill_adrift::rill_graph::backend_factory::{BackendFactory, OutputBundle};
use rill_lang::program_runner::ProgramRunner;

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();

    let (backend_name, wav_path): (String, Option<String>) = match positional.len() {
        0 => ("portaudio".into(), None),
        1 => {
            let v = positional[0].as_str();
            if v.ends_with(".wav") || std::path::Path::new(v).is_file() {
                ("portaudio".into(), Some(v.to_string()))
            } else {
                (v.to_string(), None)
            }
        }
        _ => (positional[0].clone(), Some(positional[1].clone())),
    };

    // Compile rill-lang DSL: a sampler feeding the main output.
    let reg = rill_adrift::lang_builtins::full_registry::<f32>();
    let src = "main = sampler 0.0 1.0 1.0 0.0";
    let engine = rill_lang::compile_graph::<f32>(src, &reg, RATE)?;

    // Create I/O backend
    let mut bf = BackendFactory::new();
    registration::register_backends(&mut bf);
    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
    be_params.insert("channels".into(), ParamValue::Int(2));
    let OutputBundle { driver, playback } = bf
        .create_output(&backend_name, &be_params)
        .expect("create output backend");

    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();
    let backend_display = backend_name.clone();

    let signal_thread = std::thread::spawn(move || {
        let mut runner = ProgramRunner::new(engine, None, BUF);
        runner.wire_backends(None, Some(playback));
        runner.run_with_driver(driver, t_run).ok();
    });

    let r = running.clone();
    let handle = signal_thread.thread().clone();
    let input_thread = std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        r.store(false, Ordering::Release);
        handle.unpark();
    });

    let wav_info = wav_path
        .as_ref()
        .map(|p| format!(" WAV: {p}"))
        .unwrap_or_default();
    println!("Playing through {backend_display} backend.{wav_info} Press Enter to stop.");

    input_thread.join().ok();
    signal_thread.join().ok();
    println!("Stopped.");
    Ok(())
}
