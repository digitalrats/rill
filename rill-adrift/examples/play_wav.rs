//! Play a WAV file through the rill audio graph with optional low-pass filter.
//!
//! Usage:
//!   cargo run --example play_wav -- [backend] [wav_path]
//!
//! Backend: "portaudio" (default), "alsa", "pipewire", "jack", "null"

//! Usage:
//!   cargo run --example play_wav -- [backend] [wav_path]
//!   cargo run --example play_wav -- [wav_path]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::modular::{ModularConfig, ModularSystem};
use rill_adrift::sampler::wav::load_wav;

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
    let (backend_name, wav_path): (String, String) = match positional.len() {
        0 => (
            "portaudio".into(),
            crate_dir
                .join("ESW Aura Inst - LoFi Steel - C.wav")
                .to_string_lossy()
                .to_string(),
        ),
        1 => {
            let v = positional[0].as_str();
            if v.ends_with(".wav") || std::path::Path::new(v).is_file() {
                ("portaudio".into(), v.to_string())
            } else {
                (
                    v.to_string(),
                    crate_dir
                        .join("ESW Aura Inst - LoFi Steel - C.wav")
                        .to_string_lossy()
                        .to_string(),
                )
            }
        }
        _ => (positional[0].clone(), positional[1].clone()),
    };
    let backend_display = backend_name.clone();

    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();

    let audio_thread = std::thread::spawn(move || {
        let sample = load_wav(&wav_path).expect("load_wav");
        eprintln!(
            "Loaded: {} ({} ch, {} Hz, {} samples)",
            sample.name,
            sample.channels,
            sample.sample_rate,
            sample.len()
        );

        let mut be_params = std::collections::HashMap::new();
        be_params.insert("sample_rate".into(), RATE.to_string());
        be_params.insert("buffer_size".into(), BUF.to_string());
        be_params.insert("channels".into(), "2".to_string());

        let system = ModularSystem::<BUF>::new(ModularConfig {
            sample_rate: RATE,
            block_size: BUF,
            backend_name: Some(backend_name.clone()),
            backend_params: be_params,
            ..Default::default()
        });

        let mut builder = system.create_builder();

        // TODO: restore when manual construction API is re-added
        let _builder = builder;
        eprintln!("manual construction not yet available — use serialization via GraphDef");
    });

    let t_run = running.clone();
    let audio_handle = audio_thread.thread().clone();
    let signal_thread = std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        t_run.store(false, Ordering::Release);
        audio_handle.unpark();
    });

    println!(
        "▶ Playing through {} backend (low-pass 600 Hz). Press Enter to stop.",
        backend_display
    );

    signal_thread.join().ok();
    audio_thread.join().ok();

    println!("⏹ Stopped.");
    Ok(())
}
