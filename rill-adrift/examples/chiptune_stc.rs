//! STC file player — loads a Sound Tracker compiled module and plays it
//! through the AY-3-8910 emulator via IoControl register writes.
//!
//! Demonstrates `ModuleFactory` for registering a custom rack module
//! (the STC player) that receives ClockTick via the rack actor.
//!
//! Usage:
//!   cargo run --example chiptune_stc --features "io,lofi,portaudio,serialization" -- --file <file.stc> [backend]
//!   cargo run --example chiptune_stc --features "io,lofi,alsa,serialization" -- --file <file.stc> alsa

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::modular::serialization::{ModularSystemDef, ModuleDef, RackDef};
use rill_adrift::modular::{ModularConfig, ModularSystem};
use rill_adrift::rill_core::queues::{CommandEnum, SetParameter, SignalOrigin};
use rill_adrift::rill_core::traits::{ParamValue, ParameterId};
use rill_adrift::rill_graph::serialization::{GraphDef, NodeDef, SourceDef};
use rill_adrift::rill_patchbay::module_factory::Drain;

const BUF: usize = 256;
const RATE: f32 = 44100.0;

use rill_adrift::stc_player::StcPlayer;

// ============================================================================
// Main
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    let stc_file = args
        .iter()
        .position(|a| a == "--file")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let stc_file = match stc_file {
        Some(f) => f,
        None => {
            eprintln!("Usage: chiptune_stc --file <file.stc> [--normalize] [--no-wait] [backend]");
            eprintln!("  --file <path>    Path to .stc (Sound Tracker Compiled) file (required)");
            eprintln!("  --normalize      Apply DC offset, gain, and ceiling normalization");
            eprintln!("  --no-wait        Start playback immediately without Enter keypress");
            eprintln!("  [backend]        I/O backend name (default: portaudio)");
            std::process::exit(1);
        }
    };

    let stc_data = match std::fs::read(&stc_file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error reading STC file '{}': {}", stc_file, e);
            std::process::exit(1);
        }
    };

    let _normalize = args.iter().any(|a| a == "--normalize");
    let no_wait = args.iter().any(|a| a == "--no-wait");

    // Backend name: first positional argument that is not a known flag value
    let backend_name = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(i, a)| {
            // Skip --file and its value
            if *i > 0 && args[*i - 1] == "--file" {
                return false;
            }
            !a.starts_with('-')
        })
        .map(|(_, a)| a.clone())
        .unwrap_or_else(|| "portaudio".into());
    let backend_display = backend_name.clone();

    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
    be_params.insert("channels".into(), ParamValue::Int(1));

    let mut system = ModularSystem::<BUF>::new(ModularConfig {
        sample_rate: RATE,
        block_size: BUF,
        backend_name: None,
        backend_params: HashMap::new(),
        ..Default::default()
    });
    system.set_default_backend(&backend_name, be_params);

    // Shared flag: set by Enter keypress (or MIDI Start in the future)
    let is_playing = Arc::new(AtomicBool::new(false));
    // Set when the melody loops back to the start
    let melody_done = Arc::new(AtomicBool::new(false));

    // Register the STC player as a custom rack module
    let playing_flag = is_playing.clone();
    let melody_done_flag = melody_done.clone();
    system.module_factory_mut().register_fn(
        "stc_player",
        Drain::OsThread { interval_ms: 1 },
        move |_id, _params, graph_ref| {
            let player = RefCell::new(StcPlayer::new(stc_data.clone()));
            let gr = graph_ref.clone();
            let playing = playing_flag.clone();
            let done = melody_done_flag.clone();
            Box::new(move |msg: CommandEnum| {
                if !playing.load(Ordering::Acquire) {
                    return;
                }
                if let CommandEnum::ClockTick(tick) = msg {
                    let ms = tick.samples_since_last as f64 * 1000.0 / tick.sample_rate as f64;
                    if let Some(regs) = player.borrow_mut().step_ms(ms) {
                        let pid = ParameterId::new("register_write").unwrap();
                        // Schedule the register write sample-accurately. The graph
                        // applies it during the block whose sample range contains
                        // this position. We look ahead by one I/O quantum because
                        // this module runs asynchronously: a change reacting to a
                        // tick in the current callback can only be rendered in the
                        // next one, so `sample_pos` targets the matching block of
                        // the next callback instead of collapsing onto block 0.
                        let apply_at = tick.sample_pos + tick.io_quantum as u64;
                        gr.send(CommandEnum::SetParameter(
                            SetParameter::new(
                                "".into(),
                                pid,
                                ParamValue::Bytes(regs.to_vec()),
                                SignalOrigin::Manual,
                            )
                            .with_sample_pos(apply_at),
                        ));
                    }
                    if player.borrow().finished {
                        done.store(true, Ordering::Release);
                        playing.store(false, Ordering::Release);
                    }
                }
            })
        },
    );

    let mut source_params = HashMap::new();
    source_params.insert("param_0".into(), ParamValue::Float(1_750_000.0)); // clock
    source_params.insert("register_write".into(), ParamValue::Float(0.0)); // regs

    let def = ModularSystemDef {
        format_version: "rill/1".into(),
        sample_rate: RATE,
        block_size: BUF,
        racks: vec![RackDef {
            name: "chiptune_stc".into(),
            graph: GraphDef {
                format_version: "rill/1".to_string(),
                sample_rate: RATE,
                block_size: BUF,
                resources: vec![],
                nodes: vec![NodeDef::Source(SourceDef {
                    id: 0,
                    type_name: "rill/lofi_chip".into(),
                    name: "ay_chip".into(),
                    backend: None,
                    parameters: source_params,
                })],
                connections: vec![],
                description: Some("AY-3-8910 Chiptune — Popcorn (STC)".into()),
            },
            automatons: vec![],
            modules: vec![ModuleDef::Custom {
                type_name: "stc_player".into(),
                params: HashMap::new(),
            }],
            mappings: vec![],
            description: None,
        }],
        description: Some("AY-3-8910 Chiptune — Popcorn (STC)".into()),
    };

    // ── Launch backend immediately so PipeWire/JACK ports exist ──────────
    // Recording apps (Ardour, Audacity via pw-loopback) can connect before
    // playback starts. The STC module stays silent until is_playing = true.
    let _running_system = system.launch(&def).expect("launch system");

    println!("AY-3-8910 Chiptune — Popcorn (STC) [{backend_display}]\n");
    println!("Backend ports are live — connect your recording app now.\n");
    if !no_wait {
        println!("Press Enter to start playback...");
        println!("  (Future: MIDI Start 0xFA will also trigger playback)\n");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
    }

    // ── Start playback ──────────────────────────────────────────────────
    // In the future, this flag can be wired to MidiClockTracker::playing_flag()
    is_playing.store(true, Ordering::Release);
    println!("Playing... (waiting for melody to end)\n");

    // Poll until melody finishes
    while !melody_done.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    println!("\nMelody finished.");

    Ok(())
}
