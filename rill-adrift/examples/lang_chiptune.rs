//! rill-lang chiptune — STC file played through AY-3-8910 + lofi via rill-lang DSL.
//! Mirrors chiptune_stc.rs but replaces rill-graph with rill-lang compilation.
//!
//! Usage:
//!   cargo run --example lang_chiptune --features "io,lofi,portaudio,lang" -- --file <file.stc>
//!
//! Architecture:
//!   STC file → StcPlayer (control thread) → SetParameter("regs", Bytes)
//!     → RillGraphEngine → ay38910(1750000.0, regs) → audio

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rill_adrift::rill_core::queues::{CommandEnum, SetParameter, SignalOrigin};
use rill_adrift::rill_core::traits::{ParamValue, ParameterId};
use rill_lang::program_runner::ProgramRunner;
#[path = "stc/mod.rs"]
mod stc_player;
use stc_player::StcPlayer;

// ============================================================================
// Main
// ============================================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let stc_file = args
        .iter()
        .position(|a| a == "--file")
        .and_then(|i| args.get(i + 1))
        .expect("usage: --file <file.stc>");

    let stc_data = std::fs::read(stc_file)?;
    let no_wait = args.iter().any(|a| a == "--no-wait");

    // ── Compile rill-lang DSL ──────────────────────────────────────────────
    let src = r#"
main regs = ay38910 1750000.0 regs: lofi 8 44100 0.75 1.0 1 0 1
"#;
    let reg = rill_adrift::lang_builtins::full_registry_f32();
    let engine = rill_lang::compile_graph::<f32, 256>(src, &reg, 44100.0)?;

    // ── Backend ────────────────────────────────────────────────────────────
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

    use rill_adrift::rill_graph::backend_factory::BackendFactory;
    let mut be: BackendFactory = Default::default();
    rill_adrift::registration::register_backends(&mut be);

    let mut be_params: HashMap<String, ParamValue> = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(44100.0));
    be_params.insert("block_size".into(), ParamValue::Int(2048));
    be_params.insert("channels".into(), ParamValue::Int(1));

    let output = be
        .create_output(&backend_name, &be_params)
        .map_err(|e| format!("backend: {e}"))?;

    // ── STC module (ClockTick-driven via ProgramRunner parent_ref) ─────────
    let playing = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let stc_mailbox = Arc::new(rill_core_actor::Mailbox::<CommandEnum>::new(64));
    let stc_ref = stc_mailbox.actor_ref();
    let stc_player = RefCell::new(StcPlayer::new(stc_data.clone()));

    let stc_playing = playing.clone();
    let stc_finished = finished.clone();
    let stc_handle = engine.handle();
    let stc_thread = std::thread::spawn(move || loop {
        if let Some(CommandEnum::ClockTick(tick)) = stc_mailbox.pop() {
            if !stc_playing.load(Ordering::Acquire) {
                continue;
            }
            let ms = tick.samples_since_last as f64 * 1000.0 / tick.sample_rate as f64;
            if let Some(regs) = stc_player.borrow_mut().step_ms(ms) {
                let apply_at = tick.sample_pos + tick.io_quantum as u64;
                stc_handle.send(CommandEnum::SetParameter(
                    SetParameter::new(
                        "".into(),
                        ParameterId::new("regs").unwrap(),
                        ParamValue::Bytes(regs.to_vec()),
                        SignalOrigin::Manual,
                    )
                    .with_sample_pos(apply_at),
                ));
            }
            if stc_player.borrow().finished {
                stc_finished.store(true, Ordering::Release);
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    });

    // ── ProgramRunner — signal thread ──────────────────────────────────────
    let running = Arc::new(AtomicBool::new(true));
    let runner_running = running.clone();

    let driver = output.driver.clone();
    let playback = output.playback.clone();
    let signal_thread = std::thread::spawn(move || {
        let mut runner = ProgramRunner::new(engine, Some(stc_ref));
        runner.wire_backends(None, Some(playback));
        runner.run_with_driver(driver, runner_running).ok();
    });

    println!("AY-3-8910 Chiptune (rill-lang DSL) [{backend_display}]");
    if !no_wait {
        println!("Press Enter to start playback...\n");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).ok();
    }
    playing.store(true, Ordering::Release);

    while !finished.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    running.store(false, Ordering::SeqCst);
    output.driver.stop().ok();
    signal_thread.join().ok();
    stc_thread.join().ok();
    println!("Done.");
    Ok(())
}
