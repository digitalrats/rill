//! AY-3-8910 Chiptune — Popcorn
//!
//! Demonstrates ModularSystemDef-based system construction with
//! SequencerAutomaton + table-based Servo for AY-3-8910 register control.
//!
//! Usage:
//!   cargo run --example chiptune --features "lofi,portaudio,serialization" [portaudio]
//!   cargo run --example chiptune --features "lofi,alsa,serialization" [alsa]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::modular::serialization::ModularSystemDef;
use rill_adrift::modular::{ModularConfig, ModularSystem};
use rill_adrift::rill_core::traits::ParamValue;
use rill_adrift::rill_graph::serialization::{
    ConnectionDef, GraphDef, NodeDef, SignalKind, SinkDef, SourceDef,
};
use rill_adrift::rill_patchbay::automaton::sequencer::PlayMode;
use rill_adrift::rill_patchbay::serialization::{
    AutomatonDef, ModuleDef, PatchbayDef, ServoDef, StepDef,
};

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn ay_regs(freq: f32) -> Vec<u8> {
    let div = if freq <= 0.0 {
        0
    } else {
        (1_750_000.0 / (16.0 * freq)).max(1.0) as u16
    };
    let lo = (div & 0xFF) as u8;
    let hi = (div >> 8) as u8;
    vec![lo, hi, 0, 0, 0, 0, 0, 0x38, 10, 0, 0, 0, 0, 0]
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("portaudio")
        .to_string();
    let backend_display = backend_name.clone();

    let melody = [
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

    let mut register_table: Vec<ParamValue> = Vec::new();
    let mut step_defs: Vec<StepDef> = Vec::new();
    for &(freq, dur_ms) in &melody {
        register_table.push(ParamValue::Bytes(ay_regs(freq)));
        step_defs.push(StepDef {
            value: 0.0,
            duration: dur_ms as f64 / 1000.0,
            curve: None,
        });
    }

    let mut be_params = std::collections::HashMap::new();
    be_params.insert("sample_rate".into(), RATE.to_string());
    be_params.insert("buffer_size".into(), BUF.to_string());
    be_params.insert("channels".into(), "1".to_string());

    let def = ModularSystemDef {
        format_version: "rill/1".into(),
        sample_rate: RATE,
        block_size: BUF,
        cases: vec![rill_adrift::modular::serialization::CaseDef {
            name: "chiptune".into(),
            graph: GraphDef {
                format_version: "rill/1".into(),
                sample_rate: RATE,
                block_size: BUF,
                resources: vec![],
                nodes: vec![
                    NodeDef::Source(SourceDef {
                        id: 0,
                        type_name: "rill/lofi_input".into(),
                        name: "ay_chip".into(),
                        backend: Some("ay38910".into()),
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
                        parameters: HashMap::new(),
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
            patchbay: Some(PatchbayDef {
                automata: vec![AutomatonDef::Sequencer {
                    id: "melody".into(),
                    steps: step_defs,
                    play_mode: PlayMode::Loop,
                    tempo: 120.0,
                }],
                modules: vec![ModuleDef::Servo(ServoDef {
                    automaton_id: "melody".into(),
                    target_node: 0,
                    target_param: "io_write".into(),
                    mapping: rill_adrift::rill_patchbay::serialization::MappingType::Linear,
                    min: 0.0,
                    max: 1.0,
                    enabled: true,
                    async_interval_ms: None,
                    control_strategy: None,
                    conflict_strategy: None,
                    table: Some(register_table),
                })],
                mappings: vec![],
                osc_surface: vec![],
                description: None,
            }),
        }],
        description: Some("AY-3-8910 Chiptune — Popcorn".into()),
    };

    let config = ModularConfig {
        sample_rate: RATE,
        block_size: BUF,
        backend_name: Some(backend_name.clone()),
        backend_params: be_params,
        ..Default::default()
    };

    let system = ModularSystem::<BUF>::new(config);
    let _system = system.launch(&def).expect("launch system");

    println!("AY-3-8910 Chiptune — Popcorn");
    println!("   Backend: {}\n", backend_display);
    println!("   Press Enter to stop.\n");

    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();
    std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        t_run.store(false, Ordering::Release);
    });

    // Wait for Ctrl+C
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();

    Ok(())
}
