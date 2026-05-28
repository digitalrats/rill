//! MIDI-controlled sine synth via ModularSystem serialisation.
//!
//! Demonstrates fully declarative system construction using [`ModularSystemDef`]:
//! signal graph, MIDI sensor, and parameter mappings are all described as data.
//!
//! # MIDI Mappings
//!
//! | Control | Target | Range |
//! |---------|--------|-------|
//! | CC#1 (mod wheel) | frequency | 20 Hz – 20 kHz (Exponential) |
//! | CC#7 (volume) | amplitude | 0.0 – 1.0 (Linear) |
//! | Note On | frequency + amplitude | `midi_to_freq(note)`, `velocity / 127` |
//! | Note Off | amplitude = 0 | oscillator silenced |
//!
//! # Usage
//!
//! ```bash
//! cargo run --example midi_synth_serial --features "midi,io,portaudio,serialization"
//! # or with ALSA backend:
//! cargo run --example midi_synth_serial --features "midi,io,alsa,serialization"
//! ```

use std::collections::HashMap;

use rill_adrift::modular::serialization::{ModularSystemDef, ModuleDef, RackDef};
use rill_adrift::modular::{ModularConfig, ModularSystem};
use rill_adrift::rill_core::traits::ParamValue;
use rill_adrift::rill_graph::serialization::{
    ConnectionDef, GraphDef, NodeDef, SignalKind, SinkDef, SourceDef,
};
use rill_adrift::rill_patchbay::engine::{EventPattern, MidiNoteKind};
use rill_adrift::rill_patchbay::module_def::{MappingDef, SensorDef, TransformDef};

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("portaudio")
        .to_string();
    let backend_display = backend_name.clone();

    // ── Backend params ───────────────────────────────────────────
    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), RATE.to_string());
    be_params.insert("buffer_size".into(), BUF.to_string());
    be_params.insert("channels".into(), "2".to_string());

    // ── ModularSystemDef ─────────────────────────────────────────
    let def = ModularSystemDef {
        format_version: "rill/1".into(),
        sample_rate: RATE,
        block_size: BUF,
        racks: vec![RackDef {
            name: "midi_synth".into(),
            graph: GraphDef {
                format_version: "rill/1".into(),
                sample_rate: RATE,
                block_size: BUF,
                resources: vec![],
                nodes: vec![
                    // Sine oscillator
                    NodeDef::Source(SourceDef {
                        id: 0,
                        type_name: "rill/sine".into(),
                        name: "osc".into(),
                        backend: None,
                        parameters: [
                            ("freq".into(), ParamValue::Float(220.0)),
                            ("amp".into(), ParamValue::Float(0.0)),
                        ]
                        .into(),
                    }),
                    // Stereo output
                    NodeDef::Sink(SinkDef {
                        id: 1,
                        type_name: "rill/output".into(),
                        name: "out".into(),
                        backend: None,
                        parameters: [("channels".into(), ParamValue::Float(2.0))].into(),
                    }),
                ],
                connections: vec![
                    ConnectionDef {
                        kind: SignalKind::Signal,
                        from_node: 0,
                        from_port: 0,
                        to_node: 1,
                        to_port: 0,
                    },
                    ConnectionDef {
                        kind: SignalKind::Signal,
                        from_node: 0,
                        from_port: 0,
                        to_node: 1,
                        to_port: 1,
                    },
                ],
                description: None,
            },
            automata: vec![],
            modules: vec![
                // MIDI sensor with declarative mappings
                ModuleDef::Sensor(SensorDef::Midi {
                    backend: "midir".into(),
                    port_name: "rill-midi-synth".into(),
                    mappings: vec![
                        // CC#1 → frequency (Exponential)
                        MappingDef {
                            event_pattern: EventPattern::MidiControl {
                                channel: None,
                                controller: 1,
                            },
                            target_node: 0,
                            target_param: "freq".into(),
                            transform: TransformDef::Exponential,
                            min: 20.0,
                            max: 20000.0,
                            enabled: true,
                        },
                        // CC#7 → amplitude (Linear)
                        MappingDef {
                            event_pattern: EventPattern::MidiControl {
                                channel: None,
                                controller: 7,
                            },
                            target_node: 0,
                            target_param: "amp".into(),
                            transform: TransformDef::Linear,
                            min: 0.0,
                            max: 1.0,
                            enabled: true,
                        },
                        // Note On → frequency (midi_to_freq)
                        MappingDef {
                            event_pattern: EventPattern::MidiNote {
                                channel: None,
                                note: None,
                                kind: MidiNoteKind::Frequency,
                            },
                            target_node: 0,
                            target_param: "freq".into(),
                            transform: TransformDef::Linear,
                            min: 0.0,
                            max: 1.0,
                            enabled: true,
                        },
                        // Note On/Off → amplitude (velocity/127 or 0)
                        MappingDef {
                            event_pattern: EventPattern::MidiNote {
                                channel: None,
                                note: None,
                                kind: MidiNoteKind::Amplitude,
                            },
                            target_node: 0,
                            target_param: "amp".into(),
                            transform: TransformDef::Linear,
                            min: 0.0,
                            max: 1.0,
                            enabled: true,
                        },
                    ],
                }),
            ],
            mappings: vec![],
            description: None,
        }],
        description: Some("MIDI-controlled sine synth".into()),
    };

    // ── Launch ────────────────────────────────────────────────────
    let config = ModularConfig {
        sample_rate: RATE,
        block_size: BUF,
        backend_name: Some(backend_name.clone()),
        backend_params: be_params,
        ..Default::default()
    };

    let system = ModularSystem::<BUF>::new(config);
    let _system = system.launch(&def).expect("launch system");

    println!("MIDI-controlled sine synth (ModularSystem)");
    println!("  Backend: {}", backend_display);
    println!("  CC#1 (mod wheel) → frequency (20 Hz – 20 kHz)");
    println!("  CC#7 (volume)    → amplitude (0.0 – 1.0)");
    println!("  Note On/Off      → frequency + amplitude");
    println!();
    println!("Press Enter to stop.");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();

    println!("Shutting down.");
    Ok(())
}
