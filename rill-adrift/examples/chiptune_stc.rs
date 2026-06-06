//! STC file player — loads a Sound Tracker compiled module and plays it
//! through the AY-3-8910 emulator via IoControl register writes.
//!
//! Demonstrates `ModuleFactory` for registering a custom rack module
//! (the STC player) that receives ClockTick via the rack actor.
//!
//! Usage:
//!   cargo run --example chiptune_stc --features "lofi,portaudio,serialization" -- [backend]
//!   cargo run --example chiptune_stc --features "lofi,alsa,serialization" -- alsa

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::modular::serialization::{ModularSystemDef, ModuleDef, RackDef};
use rill_adrift::modular::{ModularConfig, ModularSystem};
use rill_adrift::rill_core::queues::{CommandEnum, SetParameter, SignalOrigin};
use rill_adrift::rill_core::traits::{NodeId, ParamValue, ParameterId, PortId};
use rill_adrift::rill_graph::serialization::{
    ConnectionDef, GraphDef, NodeDef, SignalKind, SinkDef, SourceDef,
};
use rill_adrift::rill_patchbay::module_factory::Drain;

const BUF: usize = 256;
const RATE: f32 = 44100.0;

// ============================================================================
// STC Player — Sound Tracker Compiled format player
// ============================================================================

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
    pat_ptr: usize,
    song_length: u8,
    ornament_ptrs: [Option<usize>; 16],
    sample_ptrs: [Option<usize>; 16],
    delay_counter: u8,
    position: usize,
    position_height: i8,
    int_ms: f64,
    last_regs: [u8; 14],
    ch_events: [usize; 3],
    ch_current_ornament: [Option<usize>; 3],
    ch_current_sample: [Option<usize>; 3],
    ch_sample_repeat_counter: [isize; 3],
    ch_sample_position: [usize; 3],
    ch_note_value: [u8; 3],
    ch_row_skip: [isize; 3],
    ch_row_counter: [isize; 3],
    ch_envelope_state: [i8; 3],
    finished: bool,
}

const ENVELOPE_OFF: i8 = 0;
const ENVELOPE_TRIGGERED: i8 = 1;
const ENVELOPE_ON: i8 = 2;

impl StcPlayer {
    fn new(data: Vec<u8>) -> Self {
        let delay = data[0];
        let pos_ptr = u16::from_le_bytes([data[1], data[2]]) as usize;
        let orn_ptr = u16::from_le_bytes([data[3], data[4]]) as usize;
        let pat_ptr = u16::from_le_bytes([data[5], data[6]]) as usize;

        let song_length = data[pos_ptr];

        let mut sample_ptrs: [Option<usize>; 16] = [None; 16];
        let mut samp_off: usize = 27;
        let mut sample_count: usize = 0;
        while sample_count < 16 && samp_off + 99 <= data.len() && data[samp_off] < 16 {
            let num = data[samp_off] as usize;
            if sample_ptrs[num].is_none() {
                sample_ptrs[num] = Some(samp_off + 1);
            }
            samp_off += 99;
            sample_count += 1;
        }

        let mut ornament_ptrs: [Option<usize>; 16] = [None; 16];
        let mut orn_off = orn_ptr;
        let mut orn_count: usize = 0;
        while orn_count < 16 && orn_off + 33 <= data.len() && data[orn_off] < 16 {
            let num = data[orn_off] as usize;
            if ornament_ptrs[num].is_none() {
                ornament_ptrs[num] = Some(orn_off + 1);
            }
            orn_off += 33;
            orn_count += 1;
        }

        let mut p = Self {
            data,
            delay,
            pos_ptr,
            pat_ptr,
            song_length,
            ornament_ptrs,
            sample_ptrs,
            delay_counter: 1,
            position: 0,
            position_height: 0,
            int_ms: 0.0,
            last_regs: [0; 14],
            ch_events: [0; 3],
            ch_current_ornament: [None; 3],
            ch_current_sample: [None; 3],
            ch_sample_repeat_counter: [-1; 3],
            ch_sample_position: [0; 3],
            ch_note_value: [0; 3],
            ch_row_skip: [0; 3],
            ch_row_counter: [0; 3],
            ch_envelope_state: [ENVELOPE_OFF; 3],
            finished: false,
        };

        p.ch_events[0] = usize::MAX;
        p.ch_current_ornament = [p.ornament_ptrs[0]; 3];
        p.ch_current_sample = [p.sample_ptrs[1]; 3];

        p.last_regs[7] = 0x38;
        for i in 8..11 {
            p.last_regs[i] = 15;
        }

        p
    }

    fn get_sample_pos(&mut self, ch: usize) -> usize {
        if self.ch_sample_repeat_counter[ch] != -1 {
            self.ch_sample_repeat_counter[ch] -= 1;
            let pos = self.ch_sample_position[ch];
            self.ch_sample_position[ch] = (pos + 1) & 0x1F;
            if self.ch_sample_repeat_counter[ch] == 0 {
                if let Some(samp_ptr) = self.ch_current_sample[ch] {
                    let repeat_info = samp_ptr + 0x60;
                    if repeat_info + 1 < self.data.len() {
                        let first = self.data[repeat_info];
                        let replen = self.data[repeat_info + 1];
                        if first == 0 {
                            self.ch_sample_repeat_counter[ch] = -1;
                        } else {
                            let new_pos = (first as usize).wrapping_sub(1) & 0x1F;
                            self.ch_sample_position[ch] = (new_pos + 1) & 0x1F;
                            self.ch_sample_repeat_counter[ch] = replen as isize;
                            return first as usize;
                        }
                    }
                }
            }
            return pos;
        }
        0
    }

    fn get_pitch(
        sample_pitch: u16,
        sample_pos: usize,
        note_value: u8,
        ornament_ptr: Option<usize>,
        height: i8,
        data: &[u8],
    ) -> u16 {
        let orn_delta: i16 = match ornament_ptr {
            Some(ptr) => {
                let off = ptr + sample_pos;
                if off < data.len() {
                    data[off] as i8 as i16
                } else {
                    0
                }
            }
            None => 0,
        };
        let note_idx = (note_value as i16 + orn_delta + height as i16).clamp(0, 95) as usize;
        let mut pitch = ST_TABLE[note_idx];
        if sample_pitch & 0x1000 != 0 {
            pitch = pitch.wrapping_add(sample_pitch & 0xEFFF);
        } else {
            pitch = pitch.wrapping_sub(sample_pitch);
        }
        pitch
    }

    fn step_ms(&mut self, ms: f64) -> Option<[u8; 14]> {
        const INT_MS: f64 = 1000.0 / 48.828125;
        self.int_ms += ms;
        if self.int_ms >= INT_MS {
            self.int_ms -= INT_MS;
            if self.finished {
                return Some([0u8; 14]); // silence on loop
            }
            Some(self.step_int())
        } else {
            None
        }
    }

    fn step_int(&mut self) -> [u8; 14] {
        let mut regs = [0u8; 14];
        regs.copy_from_slice(&self.last_regs);

        self.delay_counter = self.delay_counter.wrapping_sub(1);
        if self.delay_counter == 0 {
            self.delay_counter = self.delay;

            for ch in 0..3usize {
                let row_counter = &mut self.ch_row_counter[ch];
                *row_counter -= 1;
                if *row_counter < 0 {
                    *row_counter = self.ch_row_skip[ch];

                    if ch == 0 {
                        let ev_ptr = self.ch_events[0];
                        let need_advance = ev_ptr == usize::MAX
                            || (ev_ptr < self.data.len() && self.data[ev_ptr] == 0xFF);
                        if need_advance {
                            let mut pos = self.position;
                            if pos >= self.song_length as usize {
                                pos = 0;
                                self.finished = true;
                            }
                            self.position = pos + 1;
                            let pos_off = self.pos_ptr + 1 + pos * 2;
                            let pat_num = self.data[pos_off] as usize;
                            self.position_height = self.data[pos_off + 1] as i8;
                            let mut po = self.pat_ptr;
                            loop {
                                if po + 6 >= self.data.len() || self.data[po] == 0 {
                                    break;
                                }
                                let pn = self.data[po] as usize;
                                if pn == pat_num {
                                    let a_off =
                                        u16::from_le_bytes([self.data[po + 1], self.data[po + 2]])
                                            as usize;
                                    let b_off =
                                        u16::from_le_bytes([self.data[po + 3], self.data[po + 4]])
                                            as usize;
                                    let c_off =
                                        u16::from_le_bytes([self.data[po + 5], self.data[po + 6]])
                                            as usize;
                                    self.ch_events[0] = a_off;
                                    self.ch_events[1] = b_off;
                                    self.ch_events[2] = c_off;
                                    break;
                                }
                                po += 7;
                            }
                        }
                    }

                    loop {
                        let ev_off = self.ch_events[ch];
                        if ev_off >= self.data.len() {
                            break;
                        }
                        let event = self.data[ev_off];
                        self.ch_events[ch] = ev_off + 1;

                        if event < 0x60 {
                            self.ch_note_value[ch] = event;
                            self.ch_sample_position[ch] = 0;
                            self.ch_sample_repeat_counter[ch] = 0x20;
                            break;
                        } else if event < 0x70 {
                            let sn = (event - 0x60) as usize;
                            self.ch_current_sample[ch] = self.sample_ptrs[sn];
                        } else if event < 0x80 {
                            let on = (event - 0x70) as usize;
                            self.ch_current_ornament[ch] = self.ornament_ptrs[on];
                            self.ch_envelope_state[ch] = ENVELOPE_OFF;
                        } else if event == 0x80 {
                            self.ch_sample_repeat_counter[ch] = -1;
                            break;
                        } else if event == 0x81 {
                            break;
                        } else if event == 0x82 {
                            self.ch_current_ornament[ch] = self.ornament_ptrs[0];
                            self.ch_envelope_state[ch] = ENVELOPE_OFF;
                        } else if event < 0x8F {
                            regs[13] = event - 0x80;
                            regs[12] = 0;
                            let next_ev_off = self.ch_events[ch];
                            if next_ev_off < self.data.len() {
                                regs[11] = self.data[next_ev_off];
                            }
                            self.ch_events[ch] = next_ev_off + 1;
                            self.ch_envelope_state[ch] = ENVELOPE_TRIGGERED;
                            self.ch_current_ornament[ch] = self.ornament_ptrs[0];
                        } else {
                            let skip = (event - 0xA1) as isize;
                            self.ch_row_counter[ch] = skip;
                            self.ch_row_skip[ch] = skip;
                        }
                    }
                }
            }
        }

        // Render AY output for current frame
        regs[7] = 0;
        for ch in 0..3usize {
            let sample_pos = self.get_sample_pos(ch);
            let sample_is_active = ch == 0 || self.ch_sample_repeat_counter[ch] != -1;

            if sample_is_active {
                if let Some(samp_ptr) = self.ch_current_sample[ch] {
                    let step_off = samp_ptr + sample_pos * 3;
                    if step_off + 2 < self.data.len() {
                        let sd0 = self.data[step_off];
                        let sd1 = self.data[step_off + 1];
                        let sd2 = self.data[step_off + 2];
                        let mask = sd1;
                        let noise_mask: u8 = if mask & 0x80 != 0 { 0x08 } else { 0x00 };
                        let tone_mask: u8 = if mask & 0x40 != 0 { 0x01 } else { 0x00 };
                        let noise_value = mask & 0x1F;
                        let mut sample_pitch = ((sd0 as u16 & 0xF0) << 4) | sd2 as u16;
                        if mask & 0x20 != 0 {
                            sample_pitch |= 0x1000;
                        }
                        let volume = sd0 & 0x0F;

                        regs[7] |= (noise_mask | tone_mask) << ch;

                        if self.ch_sample_repeat_counter[ch] != -1 {
                            if noise_mask == 0 {
                                regs[6] = noise_value;
                            }
                            let pitch = Self::get_pitch(
                                sample_pitch,
                                sample_pos,
                                self.ch_note_value[ch],
                                self.ch_current_ornament[ch],
                                self.position_height,
                                &self.data,
                            );
                            regs[ch * 2] = pitch as u8;
                            regs[(ch * 2) + 1] = (pitch >> 8) as u8;
                            regs[8 + ch] = volume;
                        } else {
                            regs[8 + ch] = 0;
                        }

                        if self.ch_sample_repeat_counter[ch] != 0xFF
                            && self.ch_envelope_state[ch] != ENVELOPE_OFF
                        {
                            if self.ch_envelope_state[ch] != ENVELOPE_ON {
                                self.ch_envelope_state[ch] = ENVELOPE_ON;
                            } else {
                                regs[13] = 0xFF;
                            }
                            regs[8 + ch] |= 0x10;
                        }
                    }
                }
            } else {
                regs[7] |= (0x01 | 0x08) << ch;
                regs[8 + ch] = 0;
            }
        }

        self.last_regs = regs;
        regs
    }
}

// ============================================================================
// Embedded STC data
// ============================================================================

const STC_DATA: &[u8] = include_bytes!("../../../Bonysoft - Popcorn (1993).stc");

// ============================================================================
// Main
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let normalize = args.iter().any(|a| a == "--normalize");

    // Backend name: first positional argument that doesn't start with `--`
    let backend_name = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "portaudio".into());
    let backend_display = backend_name.clone();

    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), RATE.to_string());
    be_params.insert("buffer_size".into(), BUF.to_string());
    be_params.insert("channels".into(), "1".to_string());

    let mut system = ModularSystem::<BUF>::new(ModularConfig {
        sample_rate: RATE,
        block_size: BUF,
        backend_name: Some(backend_name.clone()),
        backend_params: be_params,
        ..Default::default()
    });

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
            let player = RefCell::new(StcPlayer::new(STC_DATA.to_vec()));
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
                        let pid = ParameterId::new("io_write").unwrap();
                        gr.send(CommandEnum::SetParameter(SetParameter::new(
                            PortId::param(NodeId(0), 0),
                            pid,
                            ParamValue::Bytes(regs.to_vec()),
                            SignalOrigin::Manual,
                        )));
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
    source_params.insert("bit_depth".into(), ParamValue::Int(8));
    source_params.insert("nonlinear".into(), ParamValue::Bool(false));
    source_params.insert("noise_floor".into(), ParamValue::Float(-48.0));
    if normalize {
        source_params.insert("dc_offset".into(), ParamValue::Float(0.5));
        source_params.insert("output_gain".into(), ParamValue::Float(1.0));
        source_params.insert("output_ceiling".into(), ParamValue::Float(0.8));
    }

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
                nodes: vec![
                    NodeDef::Source(SourceDef {
                        id: 0,
                        type_name: "rill/lofi_input".into(),
                        name: "ay_chip".into(),
                        backend: Some("ay38910".into()),
                        parameters: source_params,
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
                description: Some("AY-3-8910 Chiptune — Popcorn (STC)".into()),
            },
            automata: vec![],
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
    // Small settle: allow backend to register ports before printing prompt
    std::thread::sleep(std::time::Duration::from_millis(200));

    println!(
        "AY-3-8910 Chiptune — Popcorn (STC) [{backend_display}]\n"
    );
    println!("Backend ports are live — connect your recording app now.\n");
    println!("Press Enter to start playback...");
    println!("  (Future: MIDI Start 0xFA will also trigger playback)\n");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();

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
