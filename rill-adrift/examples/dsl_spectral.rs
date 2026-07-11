//! # Spectral Effects in rill-lang DSL
//!
//! Demonstrates `spectralgate` and `spectraldelay` DSL builtins,
//! combined with complex arithmetic.
//!
//! ```bash
//! cargo run --example dsl_spectral --features "lang,fft"
//! ```

use rill_adrift::lang_builtins::full_registry;
use rill_adrift::rill_core::traits::algorithm::Algorithm;
use rill_lang::compile_with;

const SR: f32 = 44100.0;

fn main() {
    let reg = full_registry::<f32>();

    // ========================================================================
    // 1. Spectral gate in DSL
    // ========================================================================
    println!("=== DSL: spectralgate ===");
    {
        let src = "_ : spectralgate 0.1 0.0";
        let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile");

        let input: Vec<f32> = (0..64)
            .map(|i| {
                let t = i as f32 / SR;
                (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.8 + (i as f32 * 0.7).sin() * 0.01
            })
            .collect();
        let mut output = vec![0.0f32; 64];
        prog.process(Some(&input), &mut output).unwrap();

        let in_rms = rms(&input);
        let out_rms = rms(&output);
        println!("  spectralgate(0.1, 0.0): RMS {in_rms:.4} → {out_rms:.4}");
    }

    // ========================================================================
    // 2. Spectral delay in DSL
    // ========================================================================
    println!("\n=== DSL: spectraldelay ===");
    {
        let src = "_ : spectraldelay 0.5 0.3";
        let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile");

        let input: Vec<f32> = (0..64)
            .map(|i| {
                let t = i as f32 / SR;
                (2.0 * std::f32::consts::PI * 300.0 * t).sin() * 0.5
            })
            .collect();
        let mut output = vec![0.0f32; 64];
        prog.process(Some(&input), &mut output).unwrap();

        // Run a second block to build up feedback
        prog.process(Some(&input), &mut output).unwrap();
        let r = rms(&output);
        println!("  spectraldelay(0.5, 0.3): block 2 RMS = {r:.4}  (should differ from input)");
    }

    // ========================================================================
    // 3. Chain: spectral gate → spectral delay
    // ========================================================================
    println!("\n=== DSL: gate → delay chain ===");
    {
        let src = "_ : spectralgate 0.05 0.0 : spectraldelay 0.4 0.2";
        let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile");

        let input: Vec<f32> = (0..64)
            .map(|i| {
                let t = i as f32 / SR;
                (2.0 * std::f32::consts::PI * 500.0 * t).sin() * 0.6
                    + (i as f32 * 0.3).sin() * 0.005
                    + (i as f32 * 1.1).sin() * 0.003
            })
            .collect();
        let mut output = vec![0.0f32; 64];
        prog.process(Some(&input), &mut output).unwrap();

        for _ in 0..4 {
            prog.process(Some(&input), &mut output).unwrap();
        }
        let r = rms(&output);
        println!("  gate → delay ×5: RMS = {r:.4}");
    }

    // ========================================================================
    // 4. Complex: norm of a complex constant
    // ========================================================================
    println!("\n=== DSL: complex → norm ===");
    {
        let src = "complex 3.0 4.0 : norm";
        let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile");
        let mut output = vec![0.0f32; 64];
        prog.process(None, &mut output).unwrap();

        // norm(3+4i) = 5
        println!("  norm(3+4i) = {:.4}  (expected 5.0)", output[0]);
    }

    // ========================================================================
    // 5. Real signal: identity → gate → delay
    // ========================================================================
    println!("\n=== DSL: _ : gate : delay ===");
    {
        let src = "_ : spectralgate 0.01 0.0 : spectraldelay 0.3 0.2";
        let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile");

        let input: Vec<f32> = (0..64)
            .map(|i| {
                let t = i as f32 / SR;
                (2.0 * std::f32::consts::PI * 250.0 * t).sin() * 0.5
            })
            .collect();
        let mut output = vec![0.0f32; 64];
        prog.process(Some(&input), &mut output).unwrap();

        let r = rms(&output);
        println!("  identity → gate → delay: RMS = {r:.4}");
    }

    println!("\nAll DSL spectral examples completed.");
}

fn rms(samples: &[f32]) -> f32 {
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}
