//! Play a WAV file — manual signal processing chain.
//!
//! Demonstrates the lowest-level approach: backend creation, direct
//! `SamplePlayer` usage, and manual signal propagation through the
//! I/O callback. No graph serialisation, no rill-lang DSL — just
//! a `SamplePlayer` feeding an I/O backend.
//!
//! Usage:
//!   cargo run --example play_wav --features "io,sampler,portaudio"
//!   cargo run --example play_wav --features "io,sampler,portaudio" -- [backend] [wav_path]
//!   cargo run --example play_wav --features "io,sampler,alsa" -- alsa myfile.wav

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_core::time::ClockTick;
use rill_core::traits::{Algorithm, ParamValue, SignalSlab};
use rill_core_dsp::generators::SamplePlayer;

use rill_adrift::backend_factory::{BackendFactory, OutputBundle};
use rill_adrift::registration;

const RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();

    let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
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

    // ── 1. Register backends ──────────────────────────────────────────
    let mut bf = BackendFactory::new();
    registration::register_backends(&mut bf);
    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("buffer_size".into(), ParamValue::Int(256));
    be_params.insert("channels".into(), ParamValue::Int(2));
    let OutputBundle { driver, playback } = bf
        .create_output(&backend_name, &be_params)
        .map_err(|e| format!("create_output: {e}"))?;

    // ── 2. Load WAV on control thread ─────────────────────────────────
    let slab: SignalSlab =
        rill_adrift::sampler::wav::load_slab(&wav_path).map_err(|e| format!("load_slab: {e}"))?;
    eprintln!("Loaded {wav_path}");

    let buffer: Vec<f32> = slab
        .channels
        .first()
        .map(|ch| ch.iter().copied().collect())
        .unwrap_or_default();

    // ── 3. Create SamplePlayer (pull-mode source) ─────────────────────
    let mut player = SamplePlayer::<f32>::new(buffer);
    Algorithm::init(&mut player, RATE);
    player.set_gate(true);
    player.set_playback_rate(1.0);

    // ── 4. Manual I/O callback: SamplePlayer → playback ───────────────
    let running = Arc::new(AtomicBool::new(true));
    let runner_running = running.clone();
    let wav_display = wav_path.clone();
    let be_display = backend_name.clone();

    let signal_thread = std::thread::spawn(move || {
        let player = RefCell::new(player);
        let block_buf = RefCell::new(vec![0.0f32; 512]);
        driver.set_callback(Box::new(move |tick: &ClockTick| {
            let n = tick.samples_since_last as usize;
            let mut buf = block_buf.borrow_mut();
            let buf_slice = &mut buf[..n];
            let _ = player.borrow_mut().process(None, buf_slice);
            for ch in 0..2 {
                playback.write_output(ch, buf_slice);
            }
        }));
        driver.run(runner_running).ok();
    });

    let r = running.clone();
    let handle = signal_thread.thread().clone();
    let input_thread = std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        r.store(false, Ordering::Release);
        handle.unpark();
    });

    println!("▶ Playing {wav_display} through {be_display} backend. Press Enter to stop.");
    input_thread.join().ok();
    signal_thread.join().ok();
    println!("⏹ Stopped.");
    Ok(())
}
