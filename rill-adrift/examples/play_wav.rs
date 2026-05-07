//! Play a WAV file through the rill audio graph with optional low-pass filter.
//!
//! Usage:
//!   cargo run --example play_wav -- [backend] [wav_path]
//!
//! Backend: "cpal" (default), "alsa", "pipewire", "jack", "null"

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::io::output::Output;
use rill_adrift::registration;
use rill_adrift::rill_digital_filters::BiquadProcessor;
use rill_adrift::runtime::{Runtime, RuntimeConfig};
use rill_adrift::sampler::player::SamplePlayerNode;
use rill_adrift::sampler::wav::load_wav;

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args.get(1).cloned().unwrap_or_else(|| "cpal".into());
    let backend_display = backend_name.clone();
    let wav_path = {
        let crate_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        args.get(2).cloned().unwrap_or_else(|| {
            crate_dir
                .join("ESW Aura Inst - LoFi Steel - C.wav")
                .to_string_lossy()
                .to_string()
        })
    };

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

        let mut rt = Runtime::<BUF>::new(RuntimeConfig::default());

        let mut params = std::collections::HashMap::new();
        params.insert(
            "sample_rate".into(),
            rill_core::ParamValue::Int(RATE as i32),
        );
        params.insert("buffer_size".into(), rill_core::ParamValue::Int(BUF as i32));
        params.insert("channels".into(), rill_core::ParamValue::Int(2));
        rt.set_default_backend(&backend_name, params);

        let mut builder = rt.create_builder();

        let mut player = SamplePlayerNode::<f32, BUF>::new();
        player.load(sample);
        player.play();
        let src = builder.add_source(Box::new(player));

        let mut filter = BiquadProcessor::<f32, BUF>::new(RATE);
        filter.set_cutoff(600.0);
        filter.set_q(1.5);
        let fx = builder.add_processor(Box::new(filter));

        let snk = builder.add_sink(Box::new(Output::<f32, BUF>::new()));

        builder.connect_signal(src, 0, fx, 0);
        builder.connect_signal(fx, 0, snk, 0);
        builder.connect_signal(src, 1, snk, 1);

        let graph = builder.build().expect("graph build");

        graph.run(t_run).ok();
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
