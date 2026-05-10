//! STC file player — loads a Sound Tracker compiled module and plays it
//! through the AY-3-8910 emulator via IoControl register writes.
//!
//! Usage:
//!   cargo run --example chiptune_stc --features "lofi,portaudio" -- [backend] [stc_path]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::rill_core::queues::{SetParameter, SignalOrigin};
use rill_adrift::rill_core::time::ClockTick;
use rill_adrift::rill_core::traits::{NodeId, ParamValue, ParameterId, PortId};
use rill_adrift::rill_core_actor::{ActorCell, ActorRef};
use rill_adrift::rill_graph::serialization::{ConnectionDef, GraphDef, NodeDef, SignalKind};
use rill_adrift::runtime::{Runtime, RuntimeConfig};

const BUF: usize = 256;
const RATE: f32 = 44100.0;

/// ST_Table — note to 12-bit tone period (from Sound Tracker source)
const ST_TABLE: [u16; 96] = [
    0x0ef8, 0x0e10, 0x0d60, 0x0c80, 0x0bd8, 0x0b28, 0x0a88, 0x09f0, 0x0960, 0x08e0, 0x0858, 0x07e0,
    0x077c, 0x0708, 0x06b0, 0x0640, 0x05ec, 0x0594, 0x0544, 0x04f8, 0x04b0, 0x0470, 0x042c, 0x03f0,
    0x03be, 0x0384, 0x0358, 0x0320, 0x02f6, 0x02ca, 0x02a2, 0x027c, 0x0258, 0x0238, 0x0216, 0x01f8,
    0x01df, 0x01c2, 0x01ac, 0x0190, 0x017b, 0x0165, 0x0151, 0x013e, 0x012c, 0x011c, 0x010b, 0x00fc,
    0x00ef, 0x00e1, 0x00d6, 0x00c8, 0x00bd, 0x00b2, 0x00a8, 0x009f, 0x0096, 0x008e, 0x0085, 0x007e,
    0x0077, 0x0070, 0x006b, 0x0064, 0x005e, 0x0059, 0x0054, 0x004f, 0x004b, 0x0047, 0x0042, 0x003f,
    0x003b, 0x0038, 0x0035, 0x0032, 0x002f, 0x002c, 0x002a, 0x0027, 0x0025, 0x0023, 0x0021, 0x001f,
    0x001d, 0x001c, 0x001a, 0x0019, 0x0017, 0x0016, 0x0015, 0x0013, 0x0012, 0x0011, 0x0010, 0x000f,
];

struct StcPlayer {
    data: Vec<u8>,
    delay: u8,
    pos_ptr: usize,
    orn_ptr: usize,
    pat_ptr: usize,
    samples: Vec<[u8; 99]>,
    pos_count: u8,
    // Player state
    current_pos: usize,
    current_row: usize,
    int_counter: u8,
    // Time accumulator for audio-rate stepping
    int_ms: f64, // accumulated ms, reset on each STC interrupt
    // AY registers
    regs: [u8; 14],
    // Per-channel state
    ch_note: [u8; 3],
    ch_sample: [u8; 3],
    ch_ornament: [u8; 3],
    ch_envelope: [u16; 3],
    ch_enabled: [bool; 3],
    ch_orn_pos: [u8; 3],
    ch_sample_pos: [u8; 3],
    ch_delay: [u8; 3],
    ch_delay_cnt: [u8; 3],
    // Pattern data offsets for current pattern
    pat_off_a: usize,
    pat_off_b: usize,
    pat_off_c: usize,
    pat_next_a: usize,
    pat_next_b: usize,
    pat_next_c: usize,
}

impl StcPlayer {
    fn new(data: Vec<u8>) -> Self {
        let delay = data[0];
        let pos_ptr = u16::from_le_bytes([data[1], data[2]]) as usize;
        let orn_ptr = u16::from_le_bytes([data[3], data[4]]) as usize;
        let pat_ptr = u16::from_le_bytes([data[5], data[6]]) as usize;

        let pos_count = data[pos_ptr];

        // Read samples
        let sample_start = 27;
        let mut samples: Vec<[u8; 99]> = Vec::new();
        let mut off = sample_start;
        while off + 99 <= data.len() && data[off] != 0 && samples.len() < 16 {
            let mut s = [0u8; 99];
            s.copy_from_slice(&data[off..off + 99]);
            samples.push(s);
            off += 99;
        }

        let mut p = Self {
            data,
            delay,
            pos_ptr,
            orn_ptr,
            pat_ptr,
            samples,
            pos_count,
            current_pos: 0,
            current_row: 0,
            int_counter: 0,
            int_ms: 0.0,
            regs: [0; 14],
            ch_note: [0; 3],
            ch_sample: [0; 3],
            ch_ornament: [0; 3],
            ch_envelope: [0; 3],
            ch_enabled: [true; 3],
            ch_orn_pos: [0; 3],
            ch_sample_pos: [0; 3],
            ch_delay: [0; 3],
            ch_delay_cnt: [0; 3],
            pat_off_a: 0,
            pat_off_b: 0,
            pat_off_c: 0,
            pat_next_a: 0,
            pat_next_b: 0,
            pat_next_c: 0,
        };
        // Enable all tone channels, noise off
        p.regs[7] = 0b11_11_10_10;
        // Default volume
        for i in 8..11 {
            p.regs[i] = 15;
        }
        // Load first position
        p.load_position(0);
        p
    }

    fn load_position(&mut self, pos_idx: usize) {
        let pos_off = self.pos_ptr + 1 + pos_idx * 2;
        let transposition = self.data[pos_off] as i8;
        let pat_num = self.data[pos_off + 1] as usize;

        // Find pattern in patterns table
        let mut po = self.pat_ptr + 1;
        loop {
            let pn = self.data[po] as usize;
            let a_off = u16::from_le_bytes([self.data[po + 1], self.data[po + 2]]) as usize;
            let b_off = u16::from_le_bytes([self.data[po + 3], self.data[po + 4]]) as usize;
            let c_off = u16::from_le_bytes([self.data[po + 5], self.data[po + 6]]) as usize;
            if pn == pat_num {
                self.pat_off_a = a_off;
                self.pat_off_b = b_off;
                self.pat_off_c = c_off;
                break;
            }
            po += 7;
            if po >= self.data.len() || self.data[po] == 0 {
                break;
            }
        }

        self.current_row = 0;
        // Reset per-channel pattern pointers
        for ch in 0..3 {
            self.ch_delay_cnt[ch] = 0;
            self.ch_delay[ch] = 0;
        }
        // Load first notes
        self.advance_channel_a(self.pat_off_a, transposition);
        self.advance_channel_b(self.pat_off_b, transposition);
        self.advance_channel_c(self.pat_off_c, transposition);
    }

    fn get_tone_period(&self, note: u8, transposition: i8) -> u16 {
        if note >= 96 {
            return 0;
        }
        let idx = (note as i16 + transposition as i16).clamp(0, 95) as usize;
        ST_TABLE[idx]
    }

    fn apply_note(&mut self, ch: u8, note: u8, transposition: i8) {
        let ci = ch as usize;
        if note < 0x60 {
            self.ch_note[ci] = note;
            let tp = self.get_tone_period(note, transposition);
            self.regs[ci * 2] = tp as u8;
            self.regs[ci * 2 + 1] = (tp >> 8) as u8;
            if self.ch_sample[ci] > 0 {
                self.ch_sample_pos[ci] = 0;
            }
            if self.ch_ornament[ci] > 0 {
                self.ch_orn_pos[ci] = 0;
            }
        }
    }

    fn advance_channel(&mut self, ch: usize, start_off: usize, transposition: i8) -> usize {
        let mut off = start_off;
        loop {
            if off >= self.data.len() {
                return off;
            }
            let b = self.data[off];
            off += 1;
            match b {
                0x00..=0x5F => {
                    self.apply_note(ch as u8, b, transposition);
                    return off; // finish position
                }
                0x60..=0x6F => {
                    self.ch_sample[ch] = b - 0x60 + 1;
                }
                0x70..=0x7F => {
                    self.ch_ornament[ch] = b - 0x70;
                }
                0x80 => {
                    self.ch_enabled[ch] = false;
                    self.regs[7] |= 1 << ch;
                    return off;
                }
                0x81 => return off, // empty, finish
                0x82 => {
                    self.ch_ornament[ch] = 0;
                }
                0x83..=0x8E => {
                    let ev = b - 0x80;
                    let ep_lo = self.data[off] as u16;
                    off += 1;
                    self.ch_envelope[ch] = (ev as u16) << 8 | ep_lo;
                    self.ch_ornament[ch] = 0;
                    self.ch_enabled[ch] = true;
                    self.regs[7] &= !(1 << ch);
                }
                0xA1..=0xFE => {
                    self.ch_delay[ch] = b - 0xA1;
                }
                0xFF => return off, // end
                _ => {}             // 0x8F-0xA0 reserved
            }
        }
    }

    fn advance_channel_a(&mut self, start_off: usize, transposition: i8) {
        self.pat_next_a = self.advance_channel(0, start_off, transposition);
    }
    fn advance_channel_b(&mut self, start_off: usize, transposition: i8) {
        self.pat_next_b = self.advance_channel(1, start_off, transposition);
    }
    fn advance_channel_c(&mut self, start_off: usize, transposition: i8) {
        self.pat_next_c = self.advance_channel(2, start_off, transposition);
    }

    /// Step from audio-rate callback. Accumulates ms and calls step_int()
    /// at the STC interrupt rate (48.828 Hz = 20.48 ms per interrupt).
    fn step_ms(&mut self, ms: f64) -> Option<[u8; 14]> {
        const INT_MS: f64 = 1000.0 / 48.828125; // 20.48 ms per STC interrupt
        self.int_ms += ms;
        if self.int_ms >= INT_MS {
            self.int_ms -= INT_MS;
            Some(self.step_int())
        } else {
            None
        }
    }

    /// Step one interrupt (48.828 Hz). Returns current AY registers.
    fn step_int(&mut self) -> [u8; 14] {
        self.int_counter += 1;

        // Check per-channel delays
        for ch in 0..3 {
            if self.ch_delay[ch] > 0 {
                self.ch_delay_cnt[ch] += 1;
                if self.ch_delay_cnt[ch] >= self.ch_delay[ch] {
                    self.ch_delay_cnt[ch] = 0;
                    // Advance this channel
                    let pos_off = self.pos_ptr + 1 + self.current_pos * 2;
                    let trans = self.data[pos_off] as i8;
                    match ch {
                        0 => self.advance_channel_a(self.pat_next_a, trans),
                        1 => self.advance_channel_b(self.pat_next_b, trans),
                        2 => self.advance_channel_c(self.pat_next_c, trans),
                        _ => {}
                    }
                }
            }
        }

        if self.int_counter >= self.delay {
            self.int_counter = 0;
            self.current_row += 1;

            // Advance all channels
            let pos_off = self.pos_ptr + 1 + self.current_pos * 2;
            let trans = self.data[pos_off] as i8;
            self.advance_channel_a(self.pat_next_a, trans);
            self.advance_channel_b(self.pat_next_b, trans);
            self.advance_channel_c(self.pat_next_c, trans);

            if self.current_row >= 64 {
                self.current_row = 0;
                self.current_pos += 1;
                if self.current_pos >= self.pos_count as usize {
                    self.current_pos = 0; // loop
                }
                self.load_position(self.current_pos);
            }
        }

        // Apply ornaments
        for ch in 0..3 {
            let ci = ch;
            if self.ch_ornament[ci] > 0 {
                let orn_num = self.ch_ornament[ci] as usize;
                let orn_off =
                    self.orn_ptr + 1 + (orn_num - 1) * 33 + 1 + self.ch_orn_pos[ci] as usize;
                if orn_off < self.data.len() {
                    let delta = self.data[orn_off] as i8;
                    let note = self.ch_note[ci] as i16 + delta as i16;
                    if note >= 0 && note < 96 {
                        let tp = ST_TABLE[note as usize];
                        self.regs[ci * 2] = tp as u8;
                        self.regs[ci * 2 + 1] = (tp >> 8) as u8;
                    }
                }
                self.ch_orn_pos[ci] = self.ch_orn_pos[ci].wrapping_add(1) % 32;
            }
        }

        // Apply samples (volume + noise/tone mask)
        for ch in 0..3 {
            let ci = ch;
            if self.ch_sample[ci] > 0 && self.ch_sample_pos[ci] < 32 {
                let sn = self.ch_sample[ci] as usize;
                if sn <= self.samples.len() {
                    let off = 1 + (sn - 1) * 99 + self.ch_sample_pos[ci] as usize * 3;
                    if off + 2 < self.samples[0].len() * self.samples.len() {
                        // volume = data & 0x0F
                        // noise_mask = (data2 >> 7) & 1
                        // tone_mask = (data2 >> 6) & 1
                        // pitch = (data << 4) low bits | data3
                    }
                }
                self.ch_sample_pos[ci] = self.ch_sample_pos[ci].wrapping_add(1) % 32;
            }
        }

        self.regs
    }
}

const STC_DATA: &[u8] = include_bytes!("../../../Bonysoft - Popcorn (1993).stc");

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

        let rt = Runtime::<BUF>::new(RuntimeConfig {
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
                NodeDef {
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
                },
                NodeDef {
                    id: 1,
                    type_name: "rill/output".into(),
                    name: "output".into(),
                    backend: None,
                    parameters: [("channels".into(), ParamValue::Float(1.0))].into(),
                },
            ],
            connections: vec![ConnectionDef {
                kind: SignalKind::Signal,
                from_node: 0,
                from_port: 0,
                to_node: 1,
                to_port: 0,
            }],
            description: Some("AY-3-8910 Chiptune — Popcorn (STC)".into()),
        };

        let mut builder = rt.create_builder();
        def.populate(&mut builder).expect("populate graph");

        let (clock_tx, clock_rx) = ActorRef::<ClockTick>::new_pair();
        builder.set_clock_tx(clock_tx);

        let mut graph = builder.build().expect("graph build");
        let handle = graph.handle().expect("actor handle");

        // STC player actor
        struct StcActor {
            player: StcPlayer,
            graph_ref: ActorRef<SetParameter>,
        }
        impl ActorCell for StcActor {
            type Msg = ClockTick;
            fn receive(&mut self, tick: ClockTick) {
                let ms = tick.samples_since_last as f64 * 1000.0 / tick.sample_rate as f64;
                if let Some(regs) = self.player.step_ms(ms) {
                    self.graph_ref.send(SetParameter::new(
                        PortId::signal_out(NodeId(0), 0),
                        ParameterId::new("io_write").unwrap(),
                        ParamValue::Bytes(regs.to_vec()),
                        SignalOrigin::Manual,
                    ));
                }
            }
        }

        let running_seq = t_run.clone();
        std::thread::spawn(move || {
            let mut sequencer = StcActor {
                player: StcPlayer::new(STC_DATA.to_vec()),
                graph_ref: handle,
            };
            while running_seq.load(Ordering::Acquire) {
                while let Some(tick) = clock_rx.pop() {
                    sequencer.receive(tick);
                }
                std::thread::yield_now();
            }
        });

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

    println!("AY-3-8910 Chiptune — Popcorn (STC)");
    println!("   Backend: {}\n", backend_display);
    println!("   Press Enter to stop.\n");

    audio_thread.join().ok();
    println!("Stopped.");
    Ok(())
}
