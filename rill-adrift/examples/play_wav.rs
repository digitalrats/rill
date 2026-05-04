//! Play a WAV file through the rill audio graph with optional low-pass filter.
//!
//! Usage:
//!   cargo run --example play_wav -- [backend] [wav_path]
//!
//! Backend: "cpal" (default), "alsa", "pipewire", "jack", "null"

use rill_adrift::io::audio_io::{AudioIo, AudioIoPtr};
use rill_adrift::io::output::AudioOutput;
#[allow(unused_imports)]
use rill_adrift::io::AudioBackend;
use rill_adrift::io::AudioConfig;
use rill_adrift::rill_core::traits::processable::NodeVariant;
use rill_adrift::rill_core::traits::Sink;
use rill_adrift::rill_core::traits::SignalNode;
use rill_adrift::rill_core::time::SystemClock;
use rill_adrift::rill_graph::GraphBuilder;
use rill_adrift::sampler::player::SamplePlayerNode;
use rill_adrift::sampler::wav::load_wav;
use rill_adrift::rill_digital_filters::BiquadProcessor;

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn create_backend(name: &str, config: AudioConfig) -> Box<dyn AudioIo> {
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
    let wav_path = args.get(2).map(|s| s.as_str()).unwrap_or(
        "ESW Aura Inst - LoFi Steel - C.wav",
    );

    let config = AudioConfig::default()
        .with_sample_rate(RATE as u32)
        .with_buffer_size(BUF as u32)
        .with_channels(2);
    let backend = create_backend(backend_name, config);
    let backend_ptr = AudioIoPtr::from_ref(&*backend);

    // ── Load WAV ──────────────────────────────────────────────────────────
    let sample = load_wav(wav_path)?;
    println!(
        "Loaded: {} ({} ch, {} Hz, {} samples)",
        sample.name, sample.channels, sample.sample_rate, sample.len()
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

    let graph = builder
        .build(Box::new(SystemClock::with_sample_rate(RATE)))
        .expect("graph build");
    let (mut nodes, topo, _tick) = graph.into_parts();

    // ── Find AudioOutput in the graph and start it ────────────────────────
    let out_idx = topo
        .iter()
        .position(|&i| nodes[i].metadata().name == "AudioOutput")
        .expect("AudioOutput in graph");

    let audio_output: &mut AudioOutput<f32, BUF> = {
        let sink = &mut nodes[out_idx];
        if let NodeVariant::Sink(ref mut s) = sink {
            // Safety: we know this is an AudioOutput because we placed it
            // in the graph as a Sink at that index. The type erasure matches.
            unsafe {
                &mut *(s.as_mut() as *mut dyn Sink<f32, BUF> as *mut AudioOutput<f32, BUF>)
            }
        } else {
            panic!("expected AudioOutput at index {out_idx}");
        }
    };

    let nodes_ptr: *mut [NodeVariant<f32, BUF>] =
        Box::leak(nodes.into_boxed_slice());

    let drain_fn: Box<dyn Fn(&mut [NodeVariant<f32, BUF>]) + Send> =
        Box::new(|_| {});

    // Start the pull model — the backend calls this callback on each audio tick.
    audio_output.start(nodes_ptr, drain_fn, RATE);

    // ── Let it play ────────────────────────────────────────────────────────
    println!(
        "▶ Playing through {} backend (low-pass 600 Hz). Press Enter to stop.",
        backend_name
    );
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    // Stop the backend's audio stream.
    let _ = backend.stop();

    // Free the leaked box before exit.
    unsafe { drop(Box::from_raw(nodes_ptr)); }
    drop(backend);

    println!("⏹ Stopped.");
    Ok(())
}
