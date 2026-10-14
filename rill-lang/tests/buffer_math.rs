use rill_core::math::Transcendental;
use rill_lang::buffer_math::*;

#[test]
fn scalar_add_is_inplace() {
    let a = [1.0f32, 2.0, 3.0, 4.0];
    let b = [10.0f32, 20.0, 30.0, 40.0];
    let mut out = a;
    add_into(&mut out, &b);
    assert_eq!(out, [11.0, 22.0, 33.0, 44.0]);
    assert_eq!(a, [1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn scalar_mul_assign() {
    let mut out = [1.0f32, 2.0, 3.0];
    mul_into(&mut out, &[2.0, 2.0, 2.0]);
    assert_eq!(out, [2.0, 4.0, 6.0]);
}

#[test]
fn unary_sin_inplace() {
    let mut out = [0.0f32, <f32 as Transcendental>::PI / 2.0];
    sin_into(&mut out);
    assert!((out[1] - 1.0).abs() < 1e-6);
}
