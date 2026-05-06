//! Play a WAV file through the rill audio graph with optional low-pass filter.
//!
//! Usage:
//!   cargo run --example play_wav -- [backend] [wav_path] [device]
//!
//! Backend: "cpal" (default), "alsa", "pipewire", "jack", "null"
//! Device:  ALSA device name (e.g. "plughw:0,0", "sysdefault:CARD=UAC232").
//!          Only used with "alsa" backend. Defaults to "default".

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::io::output::AudioOutput;
use rill_adrift::io::signal_io::IoBackendPtr;
#[allow(unused_imports)]
use rill_adrift::io::AudioBackend;
use rill_adrift::io::AudioConfig;
use rill_adrift::rill_core::io::IoBackend;
use rill_adrift::rill_core::time::SystemClock;
use rill_adrift::rill_core::traits::processable::NodeVariant;
use rill_adrift::rill_core::traits::SignalNode;
use rill_adrift::rill_core::traits::Sink;
use rill_adrift::rill_digital_filters::BiquadProcessor;
use rill_adrift::rill_graph::GraphBuilder;
use rill_adrift::sampler::player::SamplePlayerNode;
use rill_adrift::sampler::wav::load_wav;

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn create_backend(name: &str, config: AudioConfig) -> Box<dyn IoBackend<f32>> {
    match name {
        "null" => Box::new(rill_adrift::io::NullBackend::new(config)),
        #[cfg(feature = "cpal")]
        "cpal" => {
            let mut b = rill_adrift::io::CpalBackend::new(config).expect("CpalBackend::new");
            b.init().expect("CpalBackend::init");
            Box::new(b)
        }
        #[cfg(feature = "alsa")]
        "alsa" => {
            let mut b = rill_adrift::io::AlsaBackend::new(config).expect("AlsaBackend::new");
            b.init().expect("AlsaBackend::init");
            Box::new(b)
        }
        #[cfg(feature = "pipewire")]
        "pipewire" => {
            let b = rill_adrift::io::PipewireBackend::new(config).expect("PipewireBackend::new");
            Box::new(b)
        }
        #[cfg(feature = "jack")]
        "jack" => {
            let b = rill_adrift::io::JackBackend::new(config).expect("JackBackend::new");
            Box::new(b)
        }
        other => {
            let hint = match other {
                "alsa" => " (enable --features alsa)",
                "pipewire" => " (enable --features pipewire)",
                "jack" => " (enable --features jack)",
                _ => "",
            };
            eprintln!("Unknown backend: {other}{hint}");
            eprintln!("Available:");
            eprintln!("  null           # always available");
            eprintln!("  cpal           # default");
            eprintln!("  alsa           # --features alsa");
            eprintln!("  pipewire       # --features pipewire");
            eprintln!("  jack           # --features jack");
            std::process::exit(1);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args.get(1).map(|s| s.as_str()).unwrap_or("cpal");
    let wav_path = args
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or("ESW Aura Inst - LoFi Steel - C.wav");
    let device_name = args.get(3).map(|s| s.as_str());

    let mut config = AudioConfig::default()
        .with_sample_rate(RATE as u32)
        .with_buffer_size(BUF as u32)
        .with_channels(2);
    if let Some(dev) = device_name {
        config = config.with_output_device(dev);
    }
    let backend = Arc::new(create_backend(backend_name, config));
    let backend_ptr = IoBackendPtr::from_ref(&**backend);

    // ── Load WAV ──────────────────────────────────────────────────────────
    let sample = load_wav(wav_path)?;
    println!(
        "Loaded: {} ({} ch, {} Hz, {} samples)",
        sample.name,
        sample.channels,
        sample.sample_rate,
        sample.len()
    );

    // ── Build graph: SamplePlayer → Biquad → AudioOutput ─────────────────
    let mut builder = GraphBuilder::<f32, BUF>::new();

    let mut player = SamplePlayerNode::<f32, BUF>::new();
    player.load(sample);
    player.play();
    let src = builder.add_source(Box::new(player));

    let mut filter = BiquadProcessor::<f32, BUF>::new(RATE);
    filter.set_cutoff(600.0);
    filter.set_q(1.5);
    let fx = builder.add_processor(Box::new(filter));

    let mut output = AudioOutput::<f32, BUF>::new();
    output.set_backend(backend_ptr);
    output.set_active(src);
    let snk = builder.add_sink(Box::new(output));

    builder.connect_signal(src, 0, fx, 0);
    builder.connect_signal(fx, 0, snk, 0);
    builder.connect_signal(src, 1, snk, 1);

    let mut graph = builder
        .build(Box::new(SystemClock::with_sample_rate(RATE)), None)
        .expect("graph build");

    // ── Start the pull model ──────────────────────────────────────────────
    let topo = graph.topo_order().to_vec();
    let out_idx = topo
        .iter()
        .position(|&i| graph.nodes()[i].metadata().name == "AudioOutput")
        .expect("AudioOutput in graph");
    let src_idx = topo[0];

    let audio_output: &mut AudioOutput<f32, BUF> = {
        let n = &mut graph.nodes_mut()[out_idx];
        if let NodeVariant::Sink(ref mut s) = n {
            unsafe { &mut *(s.as_mut() as *mut dyn Sink<f32, BUF> as *mut AudioOutput<f32, BUF>) }
        } else {
            panic!("expected AudioOutput at index {out_idx}");
        }
    };

    let nodes_ptr = graph.nodes_mut() as *mut [NodeVariant<f32, BUF>];
    let drain_fn: Box<dyn Fn(&mut [NodeVariant<f32, BUF>]) + Send> = Box::new(|_| {});

    // Register the callback (AudioOutput::start sets callback only)
    audio_output.start(nodes_ptr, drain_fn, RATE);

    // Create audio thread — rill-adrift owns thread lifecycle
    let running = Arc::new(AtomicBool::new(true));
    let t_running = running.clone();
    let t_backend = backend.clone();
    let audio_thread = std::thread::spawn(move || {
        let _ = t_backend.run(t_running.clone());
        // For non-blocking backends (JACK, CPAL) run() returns immediately
        // after setup — park until stop is signaled.
        // For blocking backends (ALSA, PW) run() returns when running is
        // already false, so the park loop exits immediately.
        while t_running.load(Ordering::Acquire) {
            std::thread::park();
        }
        let _ = t_backend.stop();
    });

    // ── Let it play ────────────────────────────────────────────────────────
    println!(
        "▶ Playing through {} backend (low-pass 600 Hz). Press Enter to stop.",
        backend_name
    );
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    running.store(false, Ordering::Release);
    audio_thread.thread().unpark();
    let _ = audio_thread.join();

    // graph dropped → everything freed

    println!("⏹ Stopped.");
    Ok(())
}
