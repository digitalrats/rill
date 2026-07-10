//! AY-3-8910 Chiptune — Popcorn (rill-lang DSL version).
//!
//! Compiles an AY-3-8910 + lofi chain via rill-lang DSL.
//! A control thread drives the melody by sending register_write commands.
//!
//! Usage:
//!   cargo run --example chiptune --features "io,lang,lofi,portaudio" [portaudio]
//!   cargo run --example chiptune --features "io,lang,lofi,alsa" [alsa]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::registration;
use rill_adrift::rill_graph::backend_factory::BackendFactory;
use rill_core::queues::{CommandEnum, SetParameter, SignalOrigin};
use rill_core::traits::{NodeId, ParamValue, ParameterId, PortId};
use rill_lang::program_runner::ProgramRunner;

const BUF: usize = 512;
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let backend_name = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("portaudio")
        .to_string();
    let backend_display = backend_name.clone();

    // Compile rill-lang DSL: ay38910 chip → lofi.
    let reg = rill_adrift::lang_builtins::full_registry_f32();
    let src = "main regs = ay38910 1750000.0 regs : lofi 8 44100 0.75 1.0 1 0 1";
    let engine = rill_lang::compile_graph::<f32>(src, &reg, RATE)?;

    // I/O backend
    let mut bf = BackendFactory::new();
    registration::register_backends(&mut bf);
    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
    be_params.insert("channels".into(), ParamValue::Int(1));
    let output = bf
        .create_output(&backend_name, &be_params)
        .map_err(|e| format!("backend: {e}"))?;

    // Control thread: melody sequencer
    let playing = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let ctrl_playing = playing.clone();
    let ctrl_finished = finished.clone();
    let ctrl_handle = engine.handle();

    let ctrl_thread = std::thread::spawn(move || loop {
        if !ctrl_playing.load(Ordering::Acquire) {
            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }
        let mut snare_toggle = false;
        for (i, &(mel_freq, dur_ms)) in MELODY.iter().enumerate() {
            if !ctrl_playing.load(Ordering::Acquire) {
                return;
            }
            let bass_idx = i / 4;
            let bass_freq = BASS[bass_idx].0;
            let snare_vol = if snare_toggle { 15 } else { 0 };
            snare_toggle = !snare_toggle;
            let regs = make_regs(mel_freq, bass_freq, snare_vol);
            ctrl_handle.send(CommandEnum::SetParameter(SetParameter::new(
                PortId::param(NodeId(0), 1),
                ParameterId::new("regs").unwrap(),
                ParamValue::Bytes(regs.to_vec()),
                SignalOrigin::Manual,
            )));
            let actual_dur = dur_ms.max(10);
            std::thread::sleep(std::time::Duration::from_millis(actual_dur));
        }
        ctrl_finished.store(true, Ordering::Release);
        return;
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

    println!("AY-3-8910 Chiptune -- Popcorn [{backend_display}]");
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
    ctrl_thread.join().ok();
    println!("Done.");
    Ok(())
}
