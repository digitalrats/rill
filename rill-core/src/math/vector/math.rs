//! # Математические функции для векторов
//!
//! Реализация математических функций (sin, cos, exp, ln, sqrt и т.д.) для векторных типов.

use super::traits::{Vector, VectorTranscendental};
use crate::Transcendental;

// -----------------------------------------------------------------------------
// Функции для работы со слайсами
// -----------------------------------------------------------------------------

/// Поэлементный синус слайса
pub fn sin_slice<T: Transcendental, const N: usize, V>(input: &[T], output: &mut [T])
where
    V: VectorTranscendental<T, N>,
{
    assert_eq!(input.len(), output.len());

    let chunks = input.len() / N;
    let remainder = input.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let vec = V::load(&input[start..start + N]);
        let result = vec.sin();
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = input[start + i].sin();
        }
    }
}

/// Поэлементный косинус слайса
pub fn cos_slice<T: Transcendental, const N: usize, V>(input: &[T], output: &mut [T])
where
    V: VectorTranscendental<T, N>,
{
    assert_eq!(input.len(), output.len());

    let chunks = input.len() / N;
    let remainder = input.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let vec = V::load(&input[start..start + N]);
        let result = vec.cos();
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = input[start + i].cos();
        }
    }
}

/// Поэлементный тангенс слайса
pub fn tan_slice<T: Transcendental, const N: usize, V>(input: &[T], output: &mut [T])
where
    V: VectorTranscendental<T, N>,
{
    assert_eq!(input.len(), output.len());

    let chunks = input.len() / N;
    let remainder = input.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let vec = V::load(&input[start..start + N]);
        let result = vec.tan();
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = input[start + i].tan();
        }
    }
}

/// Поэлементная экспонента слайса
pub fn exp_slice<T: Transcendental, const N: usize, V>(input: &[T], output: &mut [T])
where
    V: VectorTranscendental<T, N>,
{
    assert_eq!(input.len(), output.len());

    let chunks = input.len() / N;
    let remainder = input.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let vec = V::load(&input[start..start + N]);
        let result = vec.exp();
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = input[start + i].exp();
        }
    }
}

/// Поэлементный натуральный логарифм слайса
pub fn ln_slice<T: Transcendental, const N: usize, V>(input: &[T], output: &mut [T])
where
    V: VectorTranscendental<T, N>,
{
    assert_eq!(input.len(), output.len());

    let chunks = input.len() / N;
    let remainder = input.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let vec = V::load(&input[start..start + N]);
        let result = vec.ln();
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = input[start + i].ln();
        }
    }
}

/// Поэлементный квадратный корень слайса
pub fn sqrt_slice<T: Transcendental, const N: usize, V>(input: &[T], output: &mut [T])
where
    V: VectorTranscendental<T, N>,
{
    assert_eq!(input.len(), output.len());

    let chunks = input.len() / N;
    let remainder = input.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let vec = V::load(&input[start..start + N]);
        let result = vec.sqrt();
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = input[start + i].sqrt();
        }
    }
}

/// Поэлементный модуль слайса
pub fn abs_slice<T: Transcendental, const N: usize, V>(input: &[T], output: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(input.len(), output.len());

    let chunks = input.len() / N;
    let remainder = input.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let vec = V::load(&input[start..start + N]);
        let result = vec.abs();
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = input[start + i].abs();
        }
    }
}

/// Поэлементный минимум двух слайсов
pub fn min_slice<T: Transcendental, const N: usize, V>(a: &[T], b: &[T], output: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), output.len());

    let chunks = a.len() / N;
    let remainder = a.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec.min(&b_vec);
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = a[start + i].min(b[start + i]);
        }
    }
}

/// Поэлементный максимум двух слайсов
pub fn max_slice<T: Transcendental, const N: usize, V>(a: &[T], b: &[T], output: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), output.len());

    let chunks = a.len() / N;
    let remainder = a.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec.max(&b_vec);
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = a[start + i].max(b[start + i]);
        }
    }
}

/// Поэлементное ограничение слайса
pub fn clamp_slice<T: Transcendental, const N: usize, V>(
    input: &[T],
    min: &[T],
    max: &[T],
    output: &mut [T],
) where
    V: Vector<T, N>,
{
    assert_eq!(input.len(), min.len());
    assert_eq!(input.len(), max.len());
    assert_eq!(input.len(), output.len());

    let chunks = input.len() / N;
    let remainder = input.len() % N;

    for i in 0..chunks {
        let start = i * N;
        let input_vec = V::load(&input[start..start + N]);
        let min_vec = V::load(&min[start..start + N]);
        let max_vec = V::load(&max[start..start + N]);
        let result = input_vec.clamp(&min_vec, &max_vec);
        result.store(&mut output[start..start + N]);
    }

    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            output[start + i] = input[start + i].clamp(min[start + i], max[start + i]);
        }
    }
}

// -----------------------------------------------------------------------------
// Тесты
// -----------------------------------------------------------------------------
// Тесты
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transcendental;

    // Тесты будут добавлены после реализации скалярных векторов
}
