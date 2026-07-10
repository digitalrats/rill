//! STC file player — Sound Tracker Compiled format player via rill-lang DSL.
//!
//! Mirrors lang_chiptune.rs: STC player in a control thread sends AY-3-8910
//! register writes to the rill-lang compiled engine.
//!
//! Usage:
//!   cargo run --example chiptune_stc --features "io,lang,lofi,portaudio" -- --file <file.stc> [backend]
//!   cargo run --example chiptune_stc --features "io,lang,lofi,alsa" -- --file <file.stc> alsa

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::lang::program_runner::ProgramRunner;
use rill_adrift::registration;
use rill_adrift::rill_graph::backend_factory::BackendFactory;
use rill_core::queues::{CommandEnum, SetParameter, SignalOrigin};
use rill_core::traits::{NodeId, ParamValue, ParameterId, PortId};

const BUF: usize = 512;
const RATE: f32 = 44100.0;

// ============================================================================
// STC Player — Sound Tracker Compiled format player
// (identical to lang_chiptune.rs engine)
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
                return Some([0u8; 14]);
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let stc_file = args
        .iter()
        .position(|a| a == "--file")
        .and_then(|i| args.get(i + 1))
        .expect("usage: --file <file.stc>");
    let stc_data = std::fs::read(stc_file)?;

    // Compile rill-lang DSL: AY-3-8910 + lofi
    let reg = rill_adrift::lang_builtins::full_registry_f32();
    let src = r#"main regs = ay38910 1750000.0 regs : lofi 8 44100 0.75 1.0 1 0 1"#;
    let engine = rill_lang::compile_graph::<f32>(src, &reg, RATE)?;

    // I/O backend
    let backend_name = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(i, a)| {
            if *i > 0 && args[*i - 1] == "--file" {
                return false;
            }
            !a.starts_with('-')
        })
        .map(|(_, a)| a.clone())
        .unwrap_or_else(|| "portaudio".into());
    let backend_display = backend_name.clone();

    let mut be: BackendFactory = Default::default();
    registration::register_backends(&mut be);
    let mut be_params: HashMap<String, ParamValue> = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("block_size".into(), ParamValue::Int(256));
    be_params.insert("channels".into(), ParamValue::Int(1));
    let output = be
        .create_output(&backend_name, &be_params)
        .map_err(|e| format!("backend: {e}"))?;

    // STC control thread
    let playing = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let stc_player = RefCell::new(StcPlayer::new(stc_data.clone()));

    let stc_playing = playing.clone();
    let stc_finished = finished.clone();
    let stc_handle = engine.handle();
    let stc_thread = std::thread::spawn(move || {
        let step_ms = 20.48; // 1000/48.828125
        loop {
            if !stc_playing.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(1));
                continue;
            }
            if let Some(regs) = stc_player.borrow_mut().step_ms(step_ms) {
                stc_handle.send(CommandEnum::SetParameter(SetParameter::new(
                    PortId::param(NodeId(0), 1),
                    ParameterId::new("regs").unwrap(),
                    ParamValue::Bytes(regs.to_vec()),
                    SignalOrigin::Manual,
                )));
            }
            if stc_player.borrow().finished {
                stc_finished.store(true, Ordering::Release);
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    // Signal thread
    let running = Arc::new(AtomicBool::new(true));
    let runner_running = running.clone();
    let driver = output.driver.clone();
    let playback = output.playback.clone();
    let sig_thread = std::thread::spawn(move || {
        let mut runner = ProgramRunner::new(engine, None, BUF);
        runner.wire_backends(None, Some(playback));
        runner.run_with_driver(driver, runner_running).ok();
    });

    println!("AY-3-8910 Chiptune -- Popcorn (STC) [{backend_display}]");
    println!("Press Enter to start playback...\n");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
    playing.store(true, Ordering::Release);

    while !finished.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    running.store(false, Ordering::SeqCst);
    output.driver.stop().ok();
    sig_thread.join().ok();
    stc_thread.join().ok();
    println!("Done.");
    Ok(())
}
