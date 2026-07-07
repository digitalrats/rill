//! # Spectral Effects Demo
//!
//! Demonstrates `SpectralGate` and `SpectralDelay` — frequency‑domain
//! effects built on real FFT with overlap‑add processing.
//!
//! ```bash
//! cargo run --example spectral_effects --features fft
//! ```

use rill_adrift::fft::effects::spectral_delay::SpectralDelay;
use rill_adrift::fft::effects::spectral_gate::SpectralGate;

const BUF: usize = 128;
const SR: f32 = 44100.0;

fn main() {
    // ========================================================================
    // 1. SpectralGate — frequency‑domain noise gate
    // ========================================================================
    println!("=== SpectralGate ===");
    let mut gate = SpectralGate::<f32, BUF>::new();
    gate.set_threshold(0.1);
    gate.set_ratio(0.0);

    // Signal: sine + low‑level noise
    let input: Vec<f32> = (0..BUF)
        .map(|i| {
            let t = i as f32 / SR;
            (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.8
                + (i as f32 * 0.7).sin() * 0.01
                + (i as f32 * 1.3).sin() * 0.005
        })
        .collect();
    let mut output = vec![0.0f32; BUF];
    gate.process(&input, &mut output);

    let input_rms = rms(&input);
    let output_rms = rms(&output);
    println!("  Input RMS:  {input_rms:.4}  →  Output RMS: {output_rms:.4}  (noise reduced)");

    // ========================================================================
    // 2. SpectralGate — passthrough (ratio = 1.0)
    // ========================================================================
    let mut gate_open = SpectralGate::<f32, BUF>::new();
    gate_open.set_threshold(0.0);
    gate_open.set_ratio(1.0);
    let mut passthrough = vec![0.0f32; BUF];
    gate_open.process(&input, &mut passthrough);

    let max_diff = input
        .iter()
        .zip(&passthrough)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    println!("  Passthrough max error: {max_diff:.6}  (expected ≈ 0)");

    // ========================================================================
    // 3. SpectralDelay — frequency‑dependent delay (shimmer)
    // ========================================================================
    println!("\n=== SpectralDelay ===");
    let mut delay = SpectralDelay::<f32, BUF, 16>::new();
    delay.set_mix(0.5);
    delay.set_feedback(0.3);

    // Process multiple blocks to hear the shimmer build up
    let test_signal: Vec<f32> = (0..BUF)
        .map(|i| {
            let t = i as f32 / SR;
            (2.0 * std::f32::consts::PI * 200.0 * t).sin() * 0.5
                + (2.0 * std::f32::consts::PI * 600.0 * t).sin() * 0.3
        })
        .collect();

    let mut out = vec![0.0f32; BUF];
    for block in 0..20 {
        delay.process(&test_signal, &mut out);
        let r = rms(&out);
        if block < 4 || block > 15 {
            println!("  Block {block:2}: RMS = {r:.4}");
        } else if block == 4 {
            println!("  ... (blocks 4-15 omitted) ...");
        }
    }

    // ========================================================================
    // 4. Chain: SpectralGate → SpectralDelay
    // ========================================================================
    println!("\n=== Chain: gate → delay ===");
    let input_chain: Vec<f32> = (0..BUF)
        .map(|i| {
            let t = i as f32 / SR;
            (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5
        })
        .collect();

    let mut gate2 = SpectralGate::<f32, BUF>::new();
    gate2.set_threshold(0.05);
    gate2.set_ratio(0.3);

    let mut delay2 = SpectralDelay::<f32, BUF, 8>::new();
    delay2.set_mix(0.4);
    delay2.set_feedback(0.2);

    let mut mid = vec![0.0f32; BUF];
    let mut final_out = vec![0.0f32; BUF];
    let mut peak = 0.0f32;

    for _ in 0..10 {
        gate2.process(&input_chain, &mut mid);
        delay2.process(&mid, &mut final_out);
        peak = peak.max(final_out.iter().fold(0.0, |m, &v| m.max(v.abs())));
    }
    println!("  Chain peak amplitude: {peak:.4}");
    for o in &final_out[..8] {
        print!("  {o:+.6} ");
    }
    println!();

    println!("\nAll spectral effect examples completed.");
}

fn rms(samples: &[f32]) -> f32 {
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}
