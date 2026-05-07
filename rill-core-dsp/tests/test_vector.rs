//! Tests for vector operations
//!
//! Tests basic arithmetic operations and math functions
//! for both scalar and SIMD (if available) representations.

use rill_core_dsp::vector::prelude::*;

#[test]
fn test_scalar_vector_basic() {
    // Create scalar vectors from slices
    let data_a = [1.0f32, 2.0, 3.0, 4.0];
    let data_b = [5.0f32, 6.0, 7.0, 8.0];

    let vec_a = ScalarVector4::load(&data_a);
    let vec_b = ScalarVector4::load(&data_b);

    // Addition
    let vec_c = vec_a + vec_b;
    let mut result = [0.0f32; 4];
    vec_c.store(&mut result);
    assert_eq!(result, [6.0, 8.0, 10.0, 12.0]);

    // Subtraction
    let vec_c = vec_a - vec_b;
    vec_c.store(&mut result);
    assert_eq!(result, [-4.0, -4.0, -4.0, -4.0]);

    // Multiplication
    let vec_c = vec_a * vec_b;
    vec_c.store(&mut result);
    assert_eq!(result, [5.0, 12.0, 21.0, 32.0]);

    // Division
    let vec_c = vec_a / vec_b;
    vec_c.store(&mut result);
    let expected = [1.0 / 5.0, 2.0 / 6.0, 3.0 / 7.0, 4.0 / 8.0];
    for i in 0..4 {
        assert!((result[i] - expected[i]).abs() < 1e-6);
    }

    // Remainder (tested with whole numbers)
    let int_a = [10.0f32, 20.0, 30.0, 40.0];
    let int_b = [3.0f32, 7.0, 11.0, 13.0];
    let vec_a = ScalarVector4::load(&int_a);
    let vec_b = ScalarVector4::load(&int_b);
    let vec_c = vec_a % vec_b;
    vec_c.store(&mut result);
    assert_eq!(result, [1.0, 6.0, 8.0, 1.0]);
}

#[test]
fn test_scalar_vector_scalar_ops() {
    // Use VectorScalarOps trait for scalar operations
    // Only methods are implemented, multiplication via operations works
    // For simplicity, use load/store with manual computation.
    let data = [2.0f32, 4.0, 6.0, 8.0];
    let vec = ScalarVector4::load(&data);

    // Addition with scalar via splat conversion
    let scalar_vec = ScalarVector4::splat(3.0);
    let vec_add = vec + scalar_vec;
    let mut result = [0.0f32; 4];
    vec_add.store(&mut result);
    assert_eq!(result, [5.0, 7.0, 9.0, 11.0]);

    // Multiplication by scalar via splat
    let scalar_vec = ScalarVector4::splat(2.0);
    let vec_mul = vec * scalar_vec;
    vec_mul.store(&mut result);
    assert_eq!(result, [4.0, 8.0, 12.0, 16.0]);

    // Combined operations
    let scalar_one = ScalarVector4::splat(1.0);
    let scalar_half = ScalarVector4::splat(0.5);
    let vec_expr = (vec + scalar_one) * scalar_half;
    vec_expr.store(&mut result);
    assert_eq!(result, [1.5, 2.5, 3.5, 4.5]);
}

#[test]
fn test_scalar_vector_math_functions() {
    let data = [0.0f32, 0.5, 1.0, 2.0];
    let vec = ScalarVector4::load(&data);

    // Sine
    let vec_sin = vec.sin();
    let mut result = [0.0f32; 4];
    vec_sin.store(&mut result);
    let expected_sin = [0.0f32.sin(), 0.5f32.sin(), 1.0f32.sin(), 2.0f32.sin()];
    for i in 0..4 {
        assert!((result[i] - expected_sin[i]).abs() < 1e-6);
    }

    // Cosine
    let vec_cos = vec.cos();
    vec_cos.store(&mut result);
    let expected_cos = [0.0f32.cos(), 0.5f32.cos(), 1.0f32.cos(), 2.0f32.cos()];
    for i in 0..4 {
        assert!((result[i] - expected_cos[i]).abs() < 1e-6);
    }

    // Absolute value (abs)
    let data_neg = [-1.0f32, 2.0, -3.0, 4.0];
    let vec_neg = ScalarVector4::load(&data_neg);
    let vec_abs = vec_neg.abs();
    vec_abs.store(&mut result);
    assert_eq!(result, [1.0, 2.0, 3.0, 4.0]);

    // Square root
    let data_sqrt = [4.0f32, 9.0, 16.0, 25.0];
    let vec_sqrt = ScalarVector4::load(&data_sqrt);
    let vec_sqrt_res = vec_sqrt.sqrt();
    vec_sqrt_res.store(&mut result);
    assert_eq!(result, [2.0, 3.0, 4.0, 5.0]);

    // Exponential
    let data_exp = [0.0f32, 1.0, 2.0, 0.0]; // last element is don't-care
    let vec_exp = ScalarVector4::load(&data_exp);
    let vec_exp_res = vec_exp.exp();
    vec_exp_res.store(&mut result);
    assert!((result[0] - 1.0).abs() < 1e-6);
    assert!((result[1] - 2.718281828459045).abs() < 1e-6);
    assert!((result[2] - 7.38905609893065).abs() < 1e-6);

    // Natural logarithm
    let data_ln = [1.0f32, 2.718281828459045, 7.38905609893065, 1.0];
    let vec_ln = ScalarVector4::load(&data_ln);
    let vec_ln_res = vec_ln.ln();
    vec_ln_res.store(&mut result);
    assert!((result[0] - 0.0).abs() < 1e-6);
    assert!((result[1] - 1.0).abs() < 1e-6);
    assert!((result[2] - 2.0).abs() < 1e-6);
}

#[test]
fn test_scalar_vector_f64() {
    // Test with f64, size 2
    let data_a = [1.0f64, 2.0];
    let data_b = [5.0f64, 6.0];

    let vec_a = ScalarVector2::load(&data_a);
    let vec_b = ScalarVector2::load(&data_b);

    let vec_c = vec_a + vec_b;
    let mut result = [0.0f64; 2];
    vec_c.store(&mut result);
    assert_eq!(result, [6.0, 8.0]);

    let scalar_vec = ScalarVector2::splat(2.0);
    let vec_c = vec_a * scalar_vec;
    vec_c.store(&mut result);
    assert_eq!(result, [2.0, 4.0]);

    // Math functions for f64
    let vec = ScalarVector2::load(&[0.0f64, 1.0]);
    let vec_sin = vec.sin();
    vec_sin.store(&mut result);
    assert!((result[0] - 0.0).abs() < 1e-12);
    assert!((result[1] - 0.8414709848078965).abs() < 1e-12);
}

#[test]
fn test_vector_splat() {
    // Test creating a vector with identical values
    let scalar = 42.0f32;
    let vec = ScalarVector4::splat(scalar);
    let mut result = [0.0f32; 4];
    vec.store(&mut result);
    assert_eq!(result, [42.0; 4]);

    let scalar = 3.14f64;
    let vec = ScalarVector2::splat(scalar);
    let mut result = [0.0f64; 2];
    vec.store(&mut result);
    assert_eq!(result, [3.14; 2]);
}

#[test]
fn test_vector_load_store() {
    // Test correct load and store behavior
    let data = [1.0f32, 2.0, 3.0, 4.0];
    let vec = ScalarVector4::load(&data);
    let mut result = [0.0f32; 4];
    vec.store(&mut result);
    assert_eq!(result, data);

    // Partial copy (first N elements)
    let data = [5.0f32, 6.0, 7.0, 8.0, 9.0];
    let vec = ScalarVector4::load(&data); // takes first 4
    vec.store(&mut result);
    assert_eq!(result, [5.0, 6.0, 7.0, 8.0]);
}

#[test]
fn test_vector_extract_insert() {
    let data = [1.0f32, 2.0, 3.0, 4.0];
    let vec = ScalarVector4::load(&data);

    assert_eq!(vec.extract(0), 1.0);
    assert_eq!(vec.extract(1), 2.0);
    assert_eq!(vec.extract(2), 3.0);
    assert_eq!(vec.extract(3), 4.0);

    let new_vec = vec.insert(2, 99.0);
    let mut result = [0.0f32; 4];
    new_vec.store(&mut result);
    assert_eq!(result, [1.0, 2.0, 99.0, 4.0]);
}

#[test]
fn test_vector_min_max_clamp() {
    let data_a = [1.0f32, 5.0, 3.0, 7.0];
    let data_b = [4.0f32, 2.0, 6.0, 0.0];
    let vec_a = ScalarVector4::load(&data_a);
    let vec_b = ScalarVector4::load(&data_b);

    // Minimum
    let vec_min = vec_a.min(&vec_b);
    let mut result = [0.0f32; 4];
    vec_min.store(&mut result);
    assert_eq!(result, [1.0, 2.0, 3.0, 0.0]);

    // Maximum
    let vec_max = vec_a.max(&vec_b);
    vec_max.store(&mut result);
    assert_eq!(result, [4.0, 5.0, 6.0, 7.0]);

    // Clamp
    let min_vec = ScalarVector4::splat(2.0);
    let max_vec = ScalarVector4::splat(5.0);
    let vec_clamp = vec_a.clamp(&min_vec, &max_vec);
    vec_clamp.store(&mut result);
    assert_eq!(result, [2.0, 5.0, 3.0, 5.0]);
}

#[test]
fn test_vector_copy_clone() {
    // Verify vector can be copied and cloned
    let data = [1.0f32, 2.0, 3.0, 4.0];
    let vec = ScalarVector4::load(&data);
    let vec_copy = vec; // Copy

    let mut result1 = [0.0f32; 4];
    let mut result2 = [0.0f32; 4];
    vec.store(&mut result1);
    vec_copy.store(&mut result2);
    assert_eq!(result1, result2);

    // Clone
    let vec_clone = vec.clone();
    let mut result3 = [0.0f32; 4];
    vec_clone.store(&mut result3);
    assert_eq!(result1, result3);
}

// Tests for Vector trait
#[test]
fn test_vector_trait() {
    use rill_core_dsp::vector::traits::Vector as _;

    let data = [1.0f32, 2.0, 3.0, 4.0];
    let vec = ScalarVector4::load(&data);

    // Trait methods
    let splat_vec = ScalarVector4::splat(5.0);
    let mut result = [0.0f32; 4];
    splat_vec.store(&mut result);
    assert_eq!(result, [5.0; 4]);

    // Verify corresponding traits are implemented
    let _ = vec + vec; // Add
    let _ = vec - vec; // Sub
    let _ = vec * vec; // Mul
    let _ = vec / vec; // Div
    let _ = vec % vec; // Rem

    let _ = vec.sin();
    let _ = vec.cos();
    let _ = vec.abs();
    let _ = vec.sqrt();
    let _ = vec.exp();
    let _ = vec.ln();
}

// Tests for SIMD vectors (if feature is enabled)
#[cfg(feature = "simd")]
mod simd_tests {
    use super::*;
    use rill_core_dsp::vector::simd;

    #[test]
    fn test_simd_vector_f32x4() {
        // Simple check that we can create a SIMD vector
        let data = [1.0f32, 2.0, 3.0, 4.0];
        let vec = simd::F32x4::load(&data);
        let mut result = [0.0f32; 4];
        vec.store(&mut result);
        assert_eq!(result, data);
    }

    #[test]
    fn test_simd_vector_f64x2() {
        let data = [1.0f64, 2.0];
        let vec = simd::F64x2::load(&data);
        let mut result = [0.0f64; 2];
        vec.store(&mut result);
        assert_eq!(result, data);
    }
}

// Tests for reduction operations (if implemented)
// (skipped for now, VectorReduce trait not implemented for scalars)
