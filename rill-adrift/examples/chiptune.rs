//! AY-3-8910 Chiptune — Popcorn
//!
//! Demonstrates `GraphDef`-based graph construction with `LofiInput` + `Ay38910Backend`.
//! The sequencer runs externally and sends register writes via the actor mailbox.
//!
//! Usage:
//!   cargo run --example chiptune --features "lofi,portaudio" [portaudio]
//!   cargo run --example chiptune --features "lofi,alsa" [alsa]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::modular::{ModularConfig, ModularSystem};
use rill_adrift::rill_core::queues::{SetParameter, SignalOrigin};
use rill_adrift::rill_core::time::ClockTick;
use rill_adrift::rill_core::traits::{NodeId, ParamValue, ParameterId, PortId};
use rill_adrift::rill_graph::serialization::{
    ConnectionDef, GraphDef, NodeDef, SignalKind, SinkDef, SourceDef,
};

const BUF: usize = 256;
const RATE: f32 = 44100.0;

fn note_to_divider(freq: f32) -> u16 {
    if freq <= 0.0 {
        0
    } else {
        (1_750_000.0 / (16.0 * freq)).max(1.0) as u16
    }
}

#[derive(Clone, Copy)]
struct Note {
    freq: f32,
    dur_ms: u64,
}

const MELODY: &[Note] = &[
    Note {
        freq: 392.0,
        dur_ms: 120,
    },
    Note {
        freq: 440.0,
        dur_ms: 120,
    },
    Note {
        freq: 392.0,
        dur_ms: 120,
    },
    Note {
        freq: 329.6,
        dur_ms: 120,
    },
    Note {
        freq: 392.0,
        dur_ms: 120,
    },
    Note {
        freq: 440.0,
        dur_ms: 120,
    },
    Note {
        freq: 392.0,
        dur_ms: 120,
    },
    Note {
        freq: 329.6,
        dur_ms: 120,
    },
    Note {
        freq: 261.6,
        dur_ms: 120,
    },
    Note {
        freq: 329.6,
        dur_ms: 120,
    },
    Note {
        freq: 261.6,
        dur_ms: 120,
    },
    Note {
        freq: 220.0,
        dur_ms: 120,
    },
    Note {
        freq: 261.6,
        dur_ms: 120,
    },
    Note {
        freq: 329.6,
        dur_ms: 120,
    },
    Note {
        freq: 261.6,
        dur_ms: 120,
    },
    Note {
        freq: 220.0,
        dur_ms: 120,
    },
    Note {
        freq: 293.7,
        dur_ms: 120,
    },
    Note {
        freq: 349.2,
        dur_ms: 120,
    },
    Note {
        freq: 293.7,
        dur_ms: 120,
    },
    Note {
        freq: 246.9,
        dur_ms: 120,
    },
    Note {
        freq: 293.7,
        dur_ms: 120,
    },
    Note {
        freq: 349.2,
        dur_ms: 120,
    },
    Note {
        freq: 293.7,
        dur_ms: 120,
    },
    Note {
        freq: 246.9,
        dur_ms: 120,
    },
];

const BASS: &[Note] = &[
    Note {
        freq: 110.0,
        dur_ms: 480,
    },
    Note {
        freq: 130.8,
        dur_ms: 480,
    },
    Note {
        freq: 98.0,
        dur_ms: 480,
    },
    Note {
        freq: 110.0,
        dur_ms: 480,
    },
];

struct Sequencer {
    regs: [u8; 16],
    mel_step: usize,
    mel_ms: f64,
    bass_step: usize,
    bass_ms: f64,
    snare: u64,
}

impl Sequencer {
    fn new() -> Self {
        Self {
            regs: [0; 16],
            mel_step: 0,
            mel_ms: 0.0,
            bass_step: 0,
            bass_ms: 0.0,
            snare: 0,
        }
    }

    fn step(&mut self, ms: f64) -> [u8; 16] {
        self.mel_ms += ms;
        if self.mel_ms >= MELODY[self.mel_step].dur_ms as f64 {
            self.mel_ms -= MELODY[self.mel_step].dur_ms as f64;
            self.mel_step = (self.mel_step + 1) % MELODY.len();
        }
        let tp = note_to_divider(MELODY[self.mel_step].freq);
        self.regs[0] = tp as u8;
        self.regs[1] = (tp >> 8) as u8;
        self.regs[8] = if MELODY[self.mel_step].freq > 0.0 {
            10
        } else {
            0
        };

        self.bass_ms += ms;
        if self.bass_ms >= BASS[self.bass_step].dur_ms as f64 {
            self.bass_ms -= BASS[self.bass_step].dur_ms as f64;
            self.bass_step = (self.bass_step + 1) % BASS.len();
        }
        let bp = note_to_divider(BASS[self.bass_step].freq);
        self.regs[2] = bp as u8;
        self.regs[3] = (bp >> 8) as u8;
        self.regs[9] = if BASS[self.bass_step].freq > 0.0 {
            8
        } else {
            0
        };

        let snare_on = (self.mel_step % 4) == 0 && self.mel_ms < 60.0;
        if snare_on && self.snare == 0 {
            self.snare = 4;
        }
        if self.snare > 0 {
            self.regs[6] = 4;
            self.regs[10] = 12;
            self.snare -= 1;
            self.regs[7] = 0b00_00_10_10;
        } else {
            self.regs[6] = 0;
            self.regs[10] = 0;
            self.regs[7] = 0b11_11_10_10;
        }

        self.regs
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args.get(1).cloned().unwrap_or_else(|| "portaudio".into());
    let backend_display = backend_name.clone();

    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();

    let audio_thread = std::thread::spawn(move || {
        let mut be_params = std::collections::HashMap::new();
        be_params.insert("sample_rate".into(), RATE.to_string());
        be_params.insert("buffer_size".into(), BUF.to_string());
        be_params.insert("channels".into(), "1".to_string());

        let system = ModularSystem::<BUF>::new(ModularConfig {
            sample_rate: RATE,
            block_size: BUF,
            backend_name: Some(backend_name.clone()),
            backend_params: be_params,
            ..Default::default()
        });

        let def = GraphDef {
            format_version: "rill/1".to_string(),
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
            description: Some("AY-3-8910 Chiptune — Popcorn".into()),
        };

        // TODO: replace manual sequencer with Servo + SequencerAutomaton
        // clock channel will be provided by ModularSystemDef
        let mut graph = system.build_graph(&def).expect("build graph");

        graph.run(t_run).ok();
    });

    let t_run = running.clone();
    let ah = audio_thread.thread().clone();
    std::thread::spawn(move || {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
        t_run.store(false, Ordering::Release);
        ah.unpark();
    });

    println!("AY-3-8910 Chiptune — Popcorn");
    println!("   Backend: {}\n", backend_display);
    println!("   Press Enter to stop.\n");

    audio_thread.join().ok();
    println!("Stopped.");
    Ok(())
}
