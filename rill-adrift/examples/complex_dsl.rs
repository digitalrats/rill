//! # Complex Numbers in rill-lang DSL
//!
//! Demonstrates complex arithmetic builtins: generator, conjugate, magnitude,
//! phase, real/imaginary extraction, multiplication, and addition.
//!
//! ```bash
//! cargo run --example complex_dsl --features lang
//! ```

use rill_adrift::lang_builtins::full_registry;
use rill_adrift::rill_core::traits::algorithm::Algorithm;
use rill_lang::compile_with;

const SR: f32 = 44100.0;

fn main() {
    let reg = full_registry::<f32>();

    // ========================================================================
    // 1. Complex generator + norm (magnitude)
    // ========================================================================
    println!("=== complex 3.0 4.0 : norm ===");
    let src = "complex 3.0 4.0 : norm";
    let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile");
    let mut out = [0.0f32; 1];
    prog.process(None, &mut out).unwrap();
    println!("  norm(3+4i) = {:.4}  (expected 5.0)", out[0]);

    // ========================================================================
    // 2. Conjugate
    // ========================================================================
    println!("\n=== complex 3.0 4.0 : conj : re / im ===");
    let src = "complex 3.0 4.0 : conj : re";
    let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile");
    let mut out = [0.0f32; 1];
    prog.process(None, &mut out).unwrap();
    println!("  re(conj(3+4i)) = {:.4}  (expected 3.0)", out[0]);

    let src = "complex 3.0 4.0 : conj : im";
    let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile");
    let mut out = [0.0f32; 1];
    prog.process(None, &mut out).unwrap();
    println!("  im(conj(3+4i)) = {:.4}  (expected -4.0)", out[0]);

    // ========================================================================
    // 3. Phase (argument)
    // ========================================================================
    println!("\n=== arg ===");
    let tests = [
        ("complex 1.0 0.0", 0.0, "real positive"),
        (
            "complex 0.0 1.0",
            std::f32::consts::PI / 2.0,
            "pure imaginary",
        ),
        ("complex -1.0 0.0", std::f32::consts::PI, "real negative"),
        ("complex 1.0 1.0", std::f32::consts::PI / 4.0, "45°"),
    ];
    for (gen, expected, desc) in &tests {
        let src = format!("{gen} : arg");
        let mut prog = compile_with::<f32>(&src, &reg, SR).expect("compile");
        let mut out = [0.0f32; 1];
        prog.process(None, &mut out).unwrap();
        println!("  arg({desc}) = {:.4}  (expected ≈ {expected:.4})", out[0]);
    }

    // ========================================================================
    // 4. Complex multiplication
    // ========================================================================
    println!("\n=== cmul — complex multiplication ===");
    let tests = [
        ("complex 1.0 0.0 , complex 2.0 3.0", "2+3i", (1, 0, 2, 3)),
        (
            "complex 0.0 1.0 , complex 0.0 1.0",
            "i×i = -1",
            (0, 1, 0, 1),
        ),
        ("complex 2.0 3.0 , complex 1.0 -1.0", "5+1i", (2, 3, 1, -1)),
    ];
    for (args, desc, _) in &tests {
        let src = format!("{args} : cmul : re");
        let mut prog = compile_with::<f32>(&src, &reg, SR).expect("compile");
        let mut out = [0.0f32; 1];
        prog.process(None, &mut out).unwrap();

        let src_im = format!("{args} : cmul : im");
        let mut prog_im = compile_with::<f32>(&src_im, &reg, SR).expect("compile");
        let mut out_im = [0.0f32; 1];
        prog_im.process(None, &mut out_im).unwrap();
        println!("  {desc}: re={:.4}, im={:.4}", out[0], out_im[0]);
    }

    // ========================================================================
    // 5. Complex addition
    // ========================================================================
    println!("\n=== cadd — complex addition ===");
    let tests = [
        ("complex 1.0 2.0 , complex 3.0 4.0", "4+6i", (1, 2, 3, 4)),
        ("complex -1.0 0.0 , complex 2.0 5.0", "1+5i", (-1, 0, 2, 5)),
    ];
    for (args, desc, _) in &tests {
        let src = format!("{args} : cadd : re");
        let mut prog = compile_with::<f32>(&src, &reg, SR).expect("compile");
        let mut out = [0.0f32; 1];
        prog.process(None, &mut out).unwrap();

        let src_im = format!("{args} : cadd : im");
        let mut prog_im = compile_with::<f32>(&src_im, &reg, SR).expect("compile");
        let mut out_im = [0.0f32; 1];
        prog_im.process(None, &mut out_im).unwrap();
        println!("  {desc}: re={:.4}, im={:.4}", out[0], out_im[0]);
    }

    // ========================================================================
    // 6. Chain: multiply two complex numbers, then get magnitude
    // ========================================================================
    println!("\n=== Chained: cmul → norm ===");
    let src = "complex 3.0 4.0 , complex 2.0 0.0 : cmul : norm";
    let mut prog = compile_with::<f32>(src, &reg, SR).expect("compile");
    let mut out = [0.0f32; 1];
    prog.process(None, &mut out).unwrap();
    println!(
        "  norm((3+4i)×(2+0i)) = {:.4}  (expected 10.0 — norm of 6+8i)",
        out[0]
    );

    println!("\nAll complex DSL examples completed.");
}
