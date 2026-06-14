//! Simple monophonic MIDI synth — sine oscillator controlled via MIDI.
//!
//! # MIDI Mappings
//!
//! | Control | Target | Range |
//! |---------|--------|-------|
//! | CC#128 (pitch bend) | frequency | 100 Hz – 4 kHz (Exponential) |
//! | CC#1 (mod wheel) | amplitude | 0.0 – 1.0 (Linear) |
//! | Note On | frequency + amplitude | `midi_to_freq(note)`, `velocity / 127` |
//! | Note Off | amplitude = 0 | oscillator silenced |
//!
//! # Usage
//!
//! ```bash
//! # Auto-selects first non-virtual MIDI port, portaudio backend:
//! cargo run --example midi_synth --features "midi,io,portaudio"
//! # Specify port by index (1) or name (KOMPLETE), alsa backend:
//! cargo run --example midi_synth --features "midi,io,alsa" -- 1 alsa
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_core::queues::{CommandEnum, SetParameter, SignalOrigin};
use rill_core::time::ClockTick;
use rill_core::traits::{NodeId, ParamValue, Params, PortId};
use rill_core_actor::ActorSystem;
use rill_graph::{GraphBuilder, NodeFactory};
use rill_io::backends::MidirBackend;
use rill_patchbay::engine::{midi_cc, NoAction, ParameterMapping, Transform};
use rill_patchbay::midi::spawn_midi_sensor;
use rill_patchbay::Servo;

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

    // ── 1. Register node types ────────────────────────
    let mut nf = NodeFactory::<f32, BUF>::new();
    registration::register_all_nodes::<BUF>(&mut nf);

    let mut builder = GraphBuilder::new(Arc::new(nf));

    // ── 2. Build signal topology: sine oscillator → stereo output ──
    let mut osc_params = Params::new(RATE);
    osc_params.insert("freq", ParamValue::Float(220.0));
    osc_params.insert("amp", ParamValue::Float(0.0));
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
            Ok(graph) => {
                let _ = graph_tx.send((sys, graph.handle()));
                let mut state = graph.into_processing_state();
                let tick = ClockTick::default();
                let _ = state.process_block(&tick);
                while r_graph.load(Ordering::Acquire) {
                    std::thread::park();
                }
            }
            Err(e) => eprintln!("graph build: {:?}", e),
        }
    });

    let (system, graph_ref) = graph_rx.recv()?;
    let system = Arc::new(system);

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

    // Additional CC mappings (non-stateful controllers)
    let mappings = vec![midi_cc(
        7,
        None,
        osc_node,
        "amplitude",
        0.0,
        1.0,
        Transform::Linear,
    )];

    // Create a servo with stateful pitch bend / mod wheel + generic mappings
    let servo_ref = Servo::new(
        "midi_servo",
        NoAction,
        osc_node,
        "", // target_param overridden per-event
        ParameterMapping::Linear,
        0.0,
        1.0,
        system.clone(),
        graph_ref.clone(),
    )
    .with_pitch_bend(128, 2.0) // CC#128 = pitch bend, ±2 semitones
    .with_mod_wheel(1) // CC#1 = mod wheel
    .with_mappings(mappings) // fallback: generic CC mappings
    .spawn(&system);

    // Spawn the MIDI sensor, pointing raw events to the servo
    spawn_midi_sensor("midi", midi_backend, &system, servo_ref);

    // ── 6. Keep alive until Enter ────────────────────────────────────
    println!("MIDI synth active (backend: {audio_backend}):");
    println!("  Pitch bend (CC#128) → ±2 semitones (stateful)");
    println!("  Mod wheel (CC#1)    → amplitude (stateful)");
    println!("  CC#7 (volume)       → amplitude (0.0 – 1.0)");
    println!("  Note On              → freq = midi_to_freq * 2^(pitch_bend/12)");
    println!("  Note Off             → amplitude = 0");
    println!();
    println!("Press Enter to stop.");

    let r = running.clone();
    let shutdown_gr = graph_ref.clone();
    std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);

        // Fade out, then let the audio stream run in silence before stopping
        let pid = rill_core::traits::ParameterId::new("amplitude").unwrap();
        shutdown_gr.send(CommandEnum::SetParameter(SetParameter::new(
            PortId::param(NodeId(osc as u32), 0),
            pid,
            ParamValue::Float(0.0),
            SignalOrigin::Manual,
        )));
        // Let smoothing + zero-crossing detection complete, then let
        // the backend run a bit more in silence before cutting power
        std::thread::sleep(std::time::Duration::from_secs(1));
        r.store(false, Ordering::Release);
    });

    while running.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    println!("Shutting down.");
    Ok(())
}
