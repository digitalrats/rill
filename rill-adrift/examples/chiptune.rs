use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::io::output::Output;
use rill_adrift::lofi::{Ay38910Backend, ClassicSystem, LofiConfig, LofiInput};
use rill_adrift::rill_core::prelude::*;
use rill_adrift::runtime::{Runtime, RuntimeConfig};

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

struct ChiptuneSource<const N: usize> {
    regs: [u8; 16],
    mel_step: usize,
    mel_ms: u64,
    bass_step: usize,
    bass_ms: u64,
    snare: u64,
    block_ms: u64,
    lofi: LofiInput<f32, N>,
}

impl<const N: usize> ChiptuneSource<N> {
    fn new() -> Self {
        let block_ms = (N as f64 * 1000.0 / RATE as f64) as u64;
        let lofi_config = LofiConfig::for_system(ClassicSystem::Custom {
            bit_depth: 8,
            sample_rate: RATE,
            nonlinear: false,
            noise_floor: -48.0,
        });
        let mut lofi = LofiInput::<f32, N>::new(lofi_config);
        lofi.set_backend(Box::new(Ay38910Backend::new(1_750_000.0, RATE)));
        Self {
            regs: [0; 16],
            mel_step: 0,
            mel_ms: 0,
            bass_step: 0,
            bass_ms: 0,
            snare: 0,
            block_ms,
            lofi,
        }
    }

    fn step(&mut self) {
        let ms = self.block_ms.max(1);

        // Channel A — Melody
        self.mel_ms += ms;
        if self.mel_ms >= MELODY[self.mel_step].dur_ms {
            self.mel_ms -= MELODY[self.mel_step].dur_ms;
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

        // Channel B — Bass
        self.bass_ms += ms;
        if self.bass_ms >= BASS[self.bass_step].dur_ms {
            self.bass_ms -= BASS[self.bass_step].dur_ms;
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

        // Channel C — Snare on beat
        let snare_on = (self.mel_step % 4) == 0 && self.mel_ms < 60;
        if snare_on && self.snare == 0 {
            self.snare = 4;
        }
        if self.snare > 0 {
            self.regs[6] = 4;
            self.regs[10] = 12;
            self.snare -= 1;
            self.regs[7] = 0b00_00_10_10; // A(tone) B(tone) C(noise+tone)
        } else {
            self.regs[6] = 0;
            self.regs[10] = 0;
            self.regs[7] = 0b11_11_10_10; // A(tone) B(tone) C(off)
        }

        self.lofi.write_to_backend(&self.regs);
    }
}

impl<const N: usize> Node<f32, N> for ChiptuneSource<N> {
    fn metadata(&self) -> NodeMetadata {
        self.lofi.metadata()
    }
    fn node_type_id(&self) -> rill_core::NodeTypeId {
        rill_core::NodeTypeId::of::<Self>()
    }
    fn init(&mut self, sr: f32) {
        self.lofi.init(sr);
    }
    fn reset(&mut self) {
        self.regs = [0; 16];
        self.lofi.reset();
    }
    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        self.lofi.get_parameter(id)
    }
    fn set_parameter(&mut self, id: &ParameterId, v: ParamValue) -> ProcessResult<()> {
        self.lofi.set_parameter(id, v)
    }
    fn id(&self) -> NodeId {
        self.lofi.id()
    }
    fn set_id(&mut self, id: NodeId) {
        self.lofi.set_id(id)
    }
    fn input_port(&self, i: usize) -> Option<&Port<f32, N>> {
        self.lofi.input_port(i)
    }
    fn input_port_mut(&mut self, i: usize) -> Option<&mut Port<f32, N>> {
        self.lofi.input_port_mut(i)
    }
    fn output_port(&self, i: usize) -> Option<&Port<f32, N>> {
        self.lofi.output_port(i)
    }
    fn output_port_mut(&mut self, i: usize) -> Option<&mut Port<f32, N>> {
        self.lofi.output_port_mut(i)
    }
    fn control_port(&self, i: usize) -> Option<&Port<f32, N>> {
        self.lofi.control_port(i)
    }
    fn control_port_mut(&mut self, i: usize) -> Option<&mut Port<f32, N>> {
        self.lofi.control_port_mut(i)
    }
    fn state(&self) -> &NodeState<f32, N> {
        self.lofi.state()
    }
    fn state_mut(&mut self) -> &mut NodeState<f32, N> {
        self.lofi.state_mut()
    }
    fn num_signal_inputs(&self) -> usize {
        0
    }
    fn num_signal_outputs(&self) -> usize {
        1
    }
}

impl<const N: usize> Source<f32, N> for ChiptuneSource<N> {
    fn generate(
        &mut self,
        clock: &rill_core::ClockTick,
        ctrl: &[f32],
        clk: &[rill_core::ClockTick],
    ) -> ProcessResult<()> {
        self.step();
        self.lofi.generate(clock, ctrl, clk)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args.get(1).cloned().unwrap_or_else(|| "cpal".into());
    let backend_display = backend_name.clone();

    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();

    let audio_thread = std::thread::spawn(move || {
        let mut rt = Runtime::<BUF>::new(RuntimeConfig {
            sample_rate: RATE,
            block_size: BUF,
            ..Default::default()
        });

        let mut params = std::collections::HashMap::new();
        params.insert(
            "sample_rate".into(),
            rill_core::ParamValue::Int(RATE as i32),
        );
        params.insert("buffer_size".into(), rill_core::ParamValue::Int(BUF as i32));
        params.insert("channels".into(), rill_core::ParamValue::Int(1));
        rt.set_default_backend(&backend_name, params);

        let mut builder = rt.create_builder();
        let src = builder.add_source(Box::new(ChiptuneSource::<BUF>::new()));
        let snk = builder.add_sink(Box::new(Output::<f32, BUF>::with_channels(1)));
        builder.connect_signal(src, 0, snk, 0);

        let graph = builder.build().expect("graph build");
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
