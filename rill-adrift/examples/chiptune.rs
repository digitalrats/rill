//! AY-3-8910 Chiptune — Popcorn
//!
//! Demonstrates ModularSystemDef-based system construction with
//! SequencerAutomaton + table-based Servo for AY-3-8910 register control.
//!
//! Usage:
//!   cargo run --example chiptune --features "lofi,portaudio,serialization" [portaudio]
//!   cargo run --example chiptune --features "lofi,alsa,serialization" [alsa]

/// NOTE: This example uses the legacy `ProcessingState` API.
/// For rill-lang-based examples, see: `lang_chiptune`, `complex_dsl`, `dsl_spectral`.
use std::collections::HashMap;

use rill_adrift::modular::serialization::{ModularSystemDef, ModuleDef, RackDef};
use rill_adrift::modular::{ModularConfig, ModularSystem};
use rill_adrift::rill_core::traits::ParamValue;
use rill_adrift::rill_graph::serialization::{
    ConnectionDef, GraphDef, NodeDef, SignalKind, SinkDef, SourceDef,
};
use rill_adrift::rill_patchbay::automaton::sequencer::PlayMode;
use rill_adrift::rill_patchbay::serialization::{AutomatonDef, ServoDef, StepDef};

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn note_divider(freq: f32) -> u16 {
    if freq <= 0.0 {
        0
    } else {
        (1_750_000.0 / (16.0 * freq)).max(1.0) as u16
    }
}

fn make_regs(mel_freq: f32, bass_freq: f32, snare_vol: u8) -> [u8; 16] {
    let mut regs = [0u8; 16];
    let tp = note_divider(mel_freq);
    regs[0] = tp as u8;
    regs[1] = (tp >> 8) as u8;
    regs[8] = if mel_freq > 0.0 { 10 } else { 0 };
    let bp = note_divider(bass_freq);
    regs[2] = bp as u8;
    regs[3] = (bp >> 8) as u8;
    regs[9] = if bass_freq > 0.0 { 8 } else { 0 };
    regs[10] = snare_vol;
    regs[7] = 0x38;
    regs
}

const MELODY: &[(f32, u64)] = &[
    (392.0, 120),
    (440.0, 120),
    (392.0, 120),
    (329.6, 120),
    (392.0, 120),
    (440.0, 120),
    (392.0, 120),
    (329.6, 120),
    (261.6, 120),
    (329.6, 120),
    (261.6, 120),
    (220.0, 120),
    (261.6, 120),
    (329.6, 120),
    (261.6, 120),
    (220.0, 120),
];

const BASS: &[(f32, u64)] = &[(110.0, 480), (130.8, 480), (98.0, 480), (110.0, 480)];

#[cfg(not(feature = "lang"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("portaudio")
        .to_string();
    let backend_display = backend_name.clone();

    let mut register_table: Vec<ParamValue> = Vec::new();
    let mut step_defs: Vec<StepDef> = Vec::new();
    let mut snare_toggle = false;
    for (i, &(mel_freq, dur_ms)) in MELODY.iter().enumerate() {
        let bass_idx = i / 4; // bass changes every 4 melody steps (480ms / 120ms = 4)
        let bass_freq = BASS[bass_idx].0;
        let snare_vol = if snare_toggle { 15 } else { 0 };
        snare_toggle = !snare_toggle;
        let regs = make_regs(mel_freq, bass_freq, snare_vol);
        register_table.push(ParamValue::Bytes(regs.to_vec()));
        step_defs.push(StepDef {
            duration: dur_ms as f64 / 1000.0 / (60.0 / 120.0), // ms → quarter-note beats at 120 BPM
        });
    }

    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
    be_params.insert("channels".into(), ParamValue::Int(1));

    let def = ModularSystemDef {
        format_version: "rill/1".into(),
        sample_rate: RATE,
        block_size: BUF,
        racks: vec![RackDef {
            name: "chiptune".into(),
            graph: GraphDef {
                format_version: "rill/1".into(),
                sample_rate: RATE,
                block_size: BUF,
                resources: vec![],
                nodes: vec![
                    NodeDef::Source(SourceDef {
                        id: 0,
                        type_name: "rill/lofi_chip".into(),
                        name: "ay_chip".into(),
                        backend: None,
                        parameters: [
                            ("bit_depth".into(), ParamValue::Int(8)),
                            ("nonlinear".into(), ParamValue::Bool(false)),
                            ("noise_floor".into(), ParamValue::Float(-48.0)),
                        ]
                        .into(),
                    }),
                    NodeDef::Sink(SinkDef {
                        id: 1,
                        type_name: "rill/output".into(),
                        name: "output".into(),
                        backend: None,
                        parameters: [("channels".into(), ParamValue::Float(1.0))].into(),
                    }),
                ],
                connections: vec![ConnectionDef {
                    kind: SignalKind::Signal,
                    from_node: 0,
                    from_port: 0,
                    to_node: 1,
                    to_port: 0,
                }],
                description: None,
            },
            automatons: vec![AutomatonDef::Sequencer {
                id: "melody".into(),
                steps: step_defs,
                play_mode: PlayMode::Loop,
                tempo: 120.0,
            }],
            modules: vec![ModuleDef::Servo(ServoDef {
                automaton_id: "melody".into(),
                target_node: 0,
                target_param: "register_write".into(),
                mapping: rill_adrift::rill_patchbay::serialization::MappingType::Linear,
                min: 0.0,
                max: 1.0,
                enabled: true,
                async_interval_ms: None,
                control_strategy: None,
                conflict_strategy: None,
                table: Some(register_table),
                target_anchor: None,
            })],
            mappings: vec![],
            description: None,
        }],
        description: Some("AY-3-8910 Chiptune — Popcorn".into()),
    };

    let config = ModularConfig {
        sample_rate: RATE,
        block_size: BUF,
        backend_name: None,
        backend_params: HashMap::new(),
        ..Default::default()
    };

    let mut system = ModularSystem::<BUF>::new(config);
    system.set_default_backend(&backend_name, be_params);
    let _system = system.launch(&def).expect("launch system");

    println!("AY-3-8910 Chiptune — Popcorn");
    println!("   Backend: {}\n", backend_display);
    println!("   Press Enter to stop.\n");

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();

    Ok(())
}

#[cfg(feature = "lang")]
fn main() {
    eprintln!("This example uses the legacy API and is not available with the 'lang' feature.");
    eprintln!("Use the lang examples (lang_chiptune, complex_dsl, dsl_spectral) instead.");
}
