//! Simple monophonic MIDI synth — sine oscillator controlled via MIDI.
//!
//! # MIDI Mappings
//!
//! | Control | Target | Range |
//! |---------|--------|-------|
//! | CC#14 (mod wheel) | frequency | 20 Hz – 20 kHz (Exponential) |
//! | CC#15 (volume) | amplitude | 0.0 – 1.0 (Linear) |
//! | Note On | frequency + amplitude | `midi_to_freq(note)`, `velocity / 127` |
//! | Note Off | amplitude = 0 | oscillator silenced |
//!
//! # Usage
//!
//! ```bash
//! cargo run --example midi_synth --features "midi,io,portaudio"
//! cargo run --example midi_synth --features "midi,io,alsa" -- 1 alsa
//! cargo run --example midi_synth --features "midi,io,portaudio" -- KOMPLETE portaudio
//! ```

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_core::traits::{ParamValue, Params};
use rill_core_actor::ActorSystem;
use rill_graph::backend_factory::BackendFactory;
use rill_graph::{GraphBuilder, NodeFactory};
use rill_io::backends::MidirBackend;
use rill_patchbay::engine::{midi_cc, midi_note, MidiNoteKind, Transform};
use rill_patchbay::midi::spawn_midi_sensor;

use rill_adrift::registration;

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── CLI: [midi_port] [audio_backend] ────────────────────────────
    let args: Vec<String> = std::env::args().collect();
    let midi_spec = args
        .get(1)
        .filter(|s| !s.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("0");
    let audio_backend = args
        .get(2)
        .filter(|s| !s.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("portaudio")
        .to_string();

    // Show available ports
    eprintln!("Available MIDI ports:");
    let _ = MidirBackend::list_ports("rill-probe");

    // ── 1. Register node types and backends ────────────────────────
    let mut nf = NodeFactory::<f32, BUF>::new();
    registration::register_all_nodes::<BUF>(&mut nf);
    let mut bf = BackendFactory::new();
    registration::register_backends(&mut bf);

    let mut builder = GraphBuilder::new(Arc::new(nf), Arc::new(bf));

    // ── 2. Configure backend ────────────────────────────────────────
    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
    be_params.insert("output_channels".into(), ParamValue::Int(2));
    be_params.insert("input_channels".into(), ParamValue::Int(0));
    builder.set_default_backend(audio_backend.clone(), be_params);

    // ── 3. Build signal topology: sine oscillator → stereo output ──
    let mut osc_params = Params::new(RATE);
    osc_params.insert("freq", ParamValue::Float(220.0));
    osc_params.insert("amp", ParamValue::Float(0.3));
    let osc = builder.add_node("rill/sine", &osc_params);
    let out = builder.add_node("rill/output", &Params::new(RATE));
    builder.connect_signal(osc, 0, out, 0);
    builder.connect_signal(osc, 0, out, 1);

    // ── 4. Run graph on dedicated I/O thread (Graph is !Send) ───────
    let running = Arc::new(AtomicBool::new(true));
    let (graph_tx, graph_rx) = std::sync::mpsc::channel();
    let r_graph = running.clone();

    std::thread::spawn(move || {
        let sys = ActorSystem::new();
        match builder.build(&sys) {
            Ok(mut graph) => {
                let _ = graph_tx.send((sys, graph.handle()));
                if let Err(e) = graph.run(r_graph) {
                    eprintln!("audio backend error: {e}");
                }
            }
            Err(e) => eprintln!("graph build: {:?}", e),
        }
    });

    let (system, graph_ref) = graph_rx.recv()?;

    // ── 5. MIDI sensor with declarative mappings ────────────────────
    // Connect: by name substring or numeric index
    let midi_backend: Box<dyn rill_io::midi_backend::MidiBackend> = if let Ok(idx) =
        midi_spec.parse::<usize>()
    {
        Box::new(MidirBackend::new_by_port("rill-midi-synth", idx).map_err(|e| e.to_string())?)
    } else {
        Box::new(
            MidirBackend::new_by_name("rill-midi-synth", midi_spec).map_err(|e| e.to_string())?,
        )
    };
    let osc_node = rill_core::traits::NodeId(osc as u32);

    let mappings = vec![
        midi_cc(
            14,
            None,
            osc_node,
            "freq",
            20.0,
            20000.0,
            Transform::Exponential,
        ),
        midi_cc(15, None, osc_node, "amp", 0.0, 1.0, Transform::Linear),
        midi_note(
            MidiNoteKind::Frequency,
            None,
            None,
            osc_node,
            "freq",
            0.0,
            1.0,
            Transform::Linear,
        ),
        midi_note(
            MidiNoteKind::Amplitude,
            None,
            None,
            osc_node,
            "amp",
            0.0,
            1.0,
            Transform::Linear,
        ),
    ];

    spawn_midi_sensor("midi", midi_backend, mappings, &system, graph_ref);

    // ── 6. Keep alive until Enter ────────────────────────────────────
    println!("MIDI synth active (backend: {audio_backend}):");
    println!("  CC#14 (mod wheel) → frequency (20 Hz – 20 kHz)");
    println!("  CC#15 (volume)   → amplitude (0.0 – 1.0)");
    println!("  Note On/Off      → frequency + amplitude");
    println!();
    println!("Press Enter to stop.");

    let r = running.clone();
    std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        r.store(false, Ordering::Release);
    });

    while running.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    println!("Shutting down.");
    Ok(())
}
