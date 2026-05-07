//! Example of using vector operations for DSP
//!
//! Demonstrates basic vector operations: addition, multiplication,
//! math functions, and block processing.

use rill_core_dsp::vector::prelude::*;

fn main() {
    println!("=== Vector Operations Example ===");

    // 1. Basic f32 vector operations
    println!("\n1. Basic operations (f32):");

    let data_a = [1.0f32, 2.0, 3.0, 4.0];
    let data_b = [5.0f32, 6.0, 7.0, 8.0];

    let vec_a = ScalarVector4::load(&data_a);
    let vec_b = ScalarVector4::load(&data_b);

    // Addition
    let vec_sum = vec_a + vec_b;
    let mut result = [0.0f32; 4];
    vec_sum.store(&mut result);
    println!("   Addition: {:?} + {:?} = {:?}", data_a, data_b, result);

    // Multiplication
    let vec_mul = vec_a * vec_b;
    vec_mul.store(&mut result);
    println!(
        "   Multiplication: {:?} * {:?} = {:?}",
        data_a, data_b, result
    );

    // Combined expression
    let scalar = ScalarVector4::splat(2.0);
    let vec_expr = (vec_a + scalar) * vec_b;
    vec_expr.store(&mut result);
    println!("   (a + 2) * b = {:?}", result);

    // 2. Math functions
    println!("\n2. Math functions (f32):");

    let angles = [0.0f32, 0.5, 1.0, 1.5];
    let vec_angles = ScalarVector4::load(&angles);

    let vec_sin = vec_angles.sin();
    vec_sin.store(&mut result);
    println!("   sin({:?}) = {:?}", angles, result);

    let vec_cos = vec_angles.cos();
    vec_cos.store(&mut result);
    println!("   cos({:?}) = {:?}", angles, result);

    // 3. Scalar operations via splat
    println!("\n3. Scalar operations:");

    let gain = 0.5f32;
    let vec_gain = ScalarVector4::splat(gain);
    let vec_scaled = vec_a * vec_gain;
    vec_scaled.store(&mut result);
    println!("   {:?} * {} = {:?}", data_a, gain, result);

    // 4. Block processing (DSP algorithm simulation)
    println!("\n4. Block processing:");

    // Original signal (sine wave)
    let mut signal = [0.0f32; 16];
    for i in 0..16 {
        signal[i] = (i as f32 * 0.1).sin();
    }
    println!("   Original signal: {:?}", signal);

    // Apply gain to block, 4 samples at a time
    let gain = 0.8f32;
    let gain_vec = ScalarVector4::splat(gain);
    let mut processed = [0.0f32; 16];

    for (idx, chunk) in signal.chunks_exact(4).enumerate() {
        let vec_chunk = ScalarVector4::load(chunk);
        let vec_processed = vec_chunk * gain_vec;
        // Store back
        let start = idx * 4;
        vec_processed.store(&mut processed[start..start + 4]);
    }

    println!("   After gain {}: {:?}", gain, processed);

    // 5. Using different vector sizes (f64)
    println!("\n5. f64 vectors (size 2):");

    let data_a_f64 = [1.0f64, 2.0];
    let data_b_f64 = [3.0f64, 4.0];

    let vec_a_f64 = ScalarVector2::load(&data_a_f64);
    let vec_b_f64 = ScalarVector2::load(&data_b_f64);

    let vec_sum_f64 = vec_a_f64 + vec_b_f64;
    let mut result_f64 = [0.0f64; 2];
    vec_sum_f64.store(&mut result_f64);
    println!("   {:?} + {:?} = {:?}", data_a_f64, data_b_f64, result_f64);

    // 6. Vector trait method demonstration
    println!("\n6. Vector trait methods:");

    let vec = ScalarVector4::splat(3.14);
    let mut arr = [0.0f32; 4];
    vec.store(&mut arr);
    println!("   Vector from single value 3.14: {:?}", arr);

    // Extract element
    println!("   extract(2) = {}", vec.extract(2));

    // Insert element
    let new_vec = vec.insert(1, 99.0);
    new_vec.store(&mut arr);
    println!("   insert(1, 99.0) => {:?}", arr);

    // 7. Min, max, clamp
    println!("\n7. Min, max, clamp:");

    let vec1 = ScalarVector4::load(&[1.0, 5.0, 3.0, 7.0]);
    let vec2 = ScalarVector4::load(&[4.0, 2.0, 6.0, 0.0]);

    let vec_min = vec1.min(&vec2);
    vec_min.store(&mut arr);
    println!(
        "   min({:?}, {:?}) = {:?}",
        [1.0, 5.0, 3.0, 7.0],
        [4.0, 2.0, 6.0, 0.0],
        arr
    );

    let vec_max = vec1.max(&vec2);
    vec_max.store(&mut arr);
    println!("   max(...) = {:?}", arr);

    let min_vec = ScalarVector4::splat(2.0);
    let max_vec = ScalarVector4::splat(5.0);
    let vec_clamp = vec1.clamp(&min_vec, &max_vec);
    vec_clamp.store(&mut arr);
    println!("   clamp(2.0..5.0) = {:?}", arr);

    println!("\n=== Example Complete ===");
}
