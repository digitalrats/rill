//! MIDI-controlled sine synth via rill-lang DSL.
//!
//! Demonstrates rill-lang DSL compilation with MIDI control.
//!
//! Usage:
//!   cargo run --example midi_synth_serial --features "midi,io,lang,portaudio,serialization"
//!   cargo run --example midi_synth_serial --features "midi,io,lang,alsa,serialization"

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::registration;
use rill_core::traits::{NodeId, ParamValue};
use rill_graph::backend_factory::{BackendFactory, OutputBundle};
use rill_io::backends::MidirBackend;
use rill_lang::program_runner::ProgramRunner;
use rill_patchbay::engine::{midi_cc, NoAction, ParameterMapping, Transform};
use rill_patchbay::midi::spawn_midi_sensor;
use rill_patchbay::Servo;

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args
        .get(1)
        .filter(|s| !s.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("portaudio")
        .to_string();
    let backend_display = backend_name.clone();

    // Compile rill-lang DSL: sine oscillator
    let reg = rill_adrift::lang_builtins::full_registry::<f32>();
    let src = "main = sine 220.0 0.5 0.0";
    let engine = rill_lang::compile_graph::<f32>(src, &reg, RATE)?;
    let graph_ref = engine.handle();

    // I/O backend
    let mut bf = BackendFactory::new();
    registration::register_backends(&mut bf);
    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
    be_params.insert("channels".into(), ParamValue::Int(2));
    let OutputBundle { driver, playback } = bf
        .create_output(&backend_name, &be_params)
        .expect("create output backend");

    // MIDI sensor
    let midi_backend: Box<dyn rill_io::midi_input::MidiInput> =
        Box::new(MidirBackend::new("rill-midi-synth").map_err(|e| e.to_string())?);
    let osc_node = NodeId(0);

    let mappings = vec![midi_cc(
        7,
        None,
        osc_node,
        "amplitude",
        0.0,
        1.0,
        Transform::Linear,
    )];

    let system = Arc::new(rill_core_actor::ActorSystem::new());
    let servo_ref = Servo::new(
        "midi_servo",
        NoAction,
        osc_node,
        "",
        ParameterMapping::Linear,
        0.0,
        1.0,
        system.clone(),
        graph_ref.clone(),
    )
    .with_pitch_bend(128, 2.0)
    .with_mod_wheel(1)
    .with_mappings(mappings)
    .spawn(&system);

    spawn_midi_sensor("midi", midi_backend, &system, servo_ref);

    // Signal thread
    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();
    let signal_thread = std::thread::spawn(move || {
        let mut runner = ProgramRunner::new(engine, None, BUF);
        runner.wire_backends(None, Some(playback));
        runner.run_with_driver(driver, t_run).ok();
    });

    println!("MIDI-controlled sine synth (declarative) [{backend_display}]");
    println!("  Pitch bend (CC#128) -> +/-2 semitones");
    println!("  Mod wheel (CC#1)    -> amplitude");
    println!("  CC#7 (volume)       -> amplitude");
    println!("Press Enter to stop.");

    let r = running.clone();
    let handle = signal_thread.thread().clone();
    std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        std::thread::sleep(std::time::Duration::from_secs(1));
        r.store(false, Ordering::Release);
        handle.unpark();
    });

    signal_thread.join().ok();
    println!("Shutting down.");
    Ok(())
}
