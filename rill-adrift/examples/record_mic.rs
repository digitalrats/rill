//! Record microphone input — capture via DSL + SPSC‑queue output.
//!
//! Usage:
//!   cargo run --example record_mic --features "lang,io,sampler,portaudio"
//!   cargo run --example record_mic --features "lang,io,sampler,pipewire" -- pipewire [file.wav]
//!   cargo run --example record_mic --features "lang,io,sampler,alsa" -- alsa [file.wav]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use rill_adrift::backend_factory::{BackendFactory, InputBundle};
use rill_adrift::registration;
use rill_adrift::rill_core::io::{IoPlayback, SpmcPlayback};
use rill_adrift::rill_core::queues::SpscQueue;
use rill_adrift::rill_core::traits::ParamValue;
use rill_lang::program_runner::ProgramRunner;
use rill_lang::runtime::Runtime;

const BUF: usize = 256;
const RATE: f32 = 44100.0;
const QUEUE_CAP: usize = 64;

fn write_wav(
    path: &str,
    sample_rate: u32,
    channels: u16,
    samples: &[f32],
) -> Result<(), Box<dyn std::error::Error>> {
    let spec = hound::WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        writer.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let positional: Vec<&String> = args
        .iter()
        .skip(1)
        .filter(|a| !a.starts_with("--"))
        .collect();
    let (backend_arg, out_path): (Option<&str>, &str) = match positional.len() {
        0 => (None, "output.wav"),
        1 => {
            let v = positional[0].as_str();
            if v.ends_with(".wav") || std::path::Path::new(v).is_file() {
                (None, v)
            } else {
                (Some(v), "output.wav")
            }
        }
        _ => (Some(positional[0].as_str()), positional[1].as_str()),
    };

    let backend_name = backend_arg.unwrap_or("portaudio").to_string();
    let backend_display = backend_name.clone();
    let out_path = out_path.to_string();

    let reg = rill_adrift::lang_builtins::full_registry::<f32>();
    let src = "main = _";
    let engine = rill_lang::compile_graph::<f32, BUF>(src, &reg, RATE)?;

    let mut bf = BackendFactory::new();
    registration::register_backends(&mut bf);
    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
    // `main = _` is 1 -> 1; capture mono to match the mono WAV output.
    be_params.insert("input_channels".into(), ParamValue::Int(1));
    be_params.insert("output_channels".into(), ParamValue::Int(0));
    let InputBundle { driver, capture } = bf
        .create_input(&backend_name, &be_params)
        .expect("create input backend");

    let queue = Arc::new(SpscQueue::<
        rill_adrift::rill_core::queues::TelemetryBlock<f32, 256>,
        QUEUE_CAP,
    >::new());
    let mem: Arc<dyn IoPlayback> = Arc::new(SpmcPlayback::new(queue.clone(), 1, RATE));

    let recorded = Arc::new(Mutex::new(Vec::<f32>::new()));
    let actual_rate = Arc::new(AtomicU32::new(0));
    let drain_buf = recorded.clone();
    let drain_queue = queue.clone();
    let drain_rate = actual_rate.clone();
    let drain_running = Arc::new(AtomicBool::new(true));
    let dr = drain_running.clone();
    let drain_thread = std::thread::spawn(move || {
        while dr.load(Ordering::Relaxed) {
            while let Some(block) = drain_queue.pop() {
                if drain_rate.load(Ordering::Relaxed) == 0 {
                    drain_rate.store(block.sample_rate as u32, Ordering::Relaxed);
                }
                drain_buf.lock().unwrap().extend_from_slice(&block.data);
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Final drain
        while let Some(block) = drain_queue.pop() {
            drain_buf.lock().unwrap().extend_from_slice(&block.data);
        }
    });

    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();
    let driver_ctl = driver.clone();

    let signal_thread = std::thread::spawn(move || {
        let runner = ProgramRunner::new(engine, None);
        Runtime::launch::<BUF>(driver, Some(capture), Some(mem), runner, t_run).ok();
    });

    println!("Recording from {backend_display} backend... Press Enter to stop.");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    running.store(false, Ordering::Release);
    let _ = driver_ctl.stop();
    signal_thread.join().ok();
    drain_running.store(false, Ordering::Relaxed);
    drain_thread.join().ok();

    let data = recorded.lock().unwrap();
    let total_samples = data.len();
    if total_samples == 0 {
        println!("No samples recorded (capture backend may not have delivered data).");
        return Ok(());
    }
    let max_amp = data.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    println!(
        "{} max={max_amp:.6}",
        if max_amp < 0.001 {
            "  Silence"
        } else {
            "  Signal"
        }
    );
    let wav_rate = actual_rate.load(Ordering::Relaxed).max(1) as u32;
    write_wav(&out_path, wav_rate, 1, &data)?;
    println!("  Saved: {out_path} — {total_samples} samples");
    Ok(())
}
