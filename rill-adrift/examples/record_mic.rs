//! Record microphone input — simplest rill-lang DSL pass-through capture.
//!
//! Usage:
//!   cargo run --example record_mic --features "lang,io,sampler,portaudio"
//!   cargo run --example record_mic --features "lang,io,sampler,pipewire" -- pipewire [file.wav]
//!   cargo run --example record_mic --features "lang,io,sampler,alsa" -- alsa [file.wav]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use rill_adrift::registration;
use rill_adrift::rill_core::traits::ParamValue;
use rill_adrift::rill_graph::backend_factory::{BackendFactory, InputBundle};
use rill_lang::program_runner::ProgramRunner;

const BUF: usize = 256;
const RATE: f32 = 48000.0;

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

    // Compile rill-lang DSL: pass-through identity.
    let reg = rill_adrift::lang_builtins::full_registry::<f32>();
    let src = "main = _";
    let engine = rill_lang::compile_graph::<f32>(src, &reg, RATE)?;

    // Create I/O backend
    let mut bf = BackendFactory::new();
    registration::register_backends(&mut bf);
    let mut be_params = HashMap::new();
    be_params.insert("sample_rate".into(), ParamValue::Float(RATE));
    be_params.insert("buffer_size".into(), ParamValue::Int(BUF as i32));
    be_params.insert("input_channels".into(), ParamValue::Int(2));
    be_params.insert("output_channels".into(), ParamValue::Int(0));
    let InputBundle { driver, capture } = bf
        .create_input(&backend_name, &be_params)
        .expect("create input backend");

    let recorded = Arc::new(Mutex::new(Vec::<f32>::new()));
    let running = Arc::new(AtomicBool::new(true));
    let t_run = running.clone();

    let signal_thread = std::thread::spawn(move || {
        let mut runner = ProgramRunner::new(engine, None, BUF);
        runner.wire_backends(Some(capture), None);
        runner.run_with_driver(driver, t_run).ok();
    });

    let r = running.clone();
    let handle = signal_thread.thread().clone();
    let input_thread = std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::stdin().read_line(&mut line);
        r.store(false, Ordering::Release);
        handle.unpark();
    });

    println!("Recording from {backend_display} backend... Press Enter to stop.");
    input_thread.join().ok();
    signal_thread.join().ok();

    // Save WAV
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
    write_wav(&out_path, 48000, 2, &data)?;
    println!("  Saved: {out_path} — {total_samples} samples");
    Ok(())
}
