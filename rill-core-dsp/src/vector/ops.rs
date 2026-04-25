//! # Арифметические операции для векторов
//!
//! Реализация базовых арифметических операций для векторных типов.

use rill_core::AudioNum;
use super::traits::*;

// -----------------------------------------------------------------------------
// Вспомогательные функции
// -----------------------------------------------------------------------------

/// Поэлементное сложение двух слайсов с сохранением результата в третий
pub fn add_slices<T: AudioNum, const N: usize, V>(a: &[T], b: &[T], out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), out.len());
    
    let chunks = a.len() / N;
    let remainder = a.len() % N;
    
    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec + b_vec;
        result.store(&mut out[start..start + N]);
    }
    
    // Обработка остатка
    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] + b[start + i];
        }
    }
}

/// Поэлементное вычитание двух слайсов
pub fn sub_slices<T: AudioNum, const N: usize, V>(a: &[T], b: &[T], out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), out.len());
    
    let chunks = a.len() / N;
    let remainder = a.len() % N;
    
    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec - b_vec;
        result.store(&mut out[start..start + N]);
    }
    
    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] - b[start + i];
        }
    }
}

/// Поэлементное умножение двух слайсов
pub fn mul_slices<T: AudioNum, const N: usize, V>(a: &[T], b: &[T], out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), out.len());
    
    let chunks = a.len() / N;
    let remainder = a.len() % N;
    
    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec * b_vec;
        result.store(&mut out[start..start + N]);
    }
    
    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] * b[start + i];
        }
    }
}

/// Поэлементное деление двух слайсов
pub fn div_slices<T: AudioNum, const N: usize, V>(a: &[T], b: &[T], out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), b.len());
    assert_eq!(a.len(), out.len());
    
    let chunks = a.len() / N;
    let remainder = a.len() % N;
    
    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let b_vec = V::load(&b[start..start + N]);
        let result = a_vec / b_vec;
        result.store(&mut out[start..start + N]);
    }
    
    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] / b[start + i];
        }
    }
}

/// Умножение слайса на скаляр
pub fn mul_scalar_slice<T: AudioNum, const N: usize, V>(a: &[T], scalar: T, out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), out.len());
    
    let scalar_vec = V::splat(scalar);
    let chunks = a.len() / N;
    let remainder = a.len() % N;
    
    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let result = a_vec * scalar_vec;
        result.store(&mut out[start..start + N]);
    }
    
    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] * scalar;
        }
    }
}

/// Сложение слайса со скаляром
pub fn add_scalar_slice<T: AudioNum, const N: usize, V>(a: &[T], scalar: T, out: &mut [T])
where
    V: Vector<T, N>,
{
    assert_eq!(a.len(), out.len());
    
    let scalar_vec = V::splat(scalar);
    let chunks = a.len() / N;
    let remainder = a.len() % N;
    
    for i in 0..chunks {
        let start = i * N;
        let a_vec = V::load(&a[start..start + N]);
        let result = a_vec + scalar_vec;
        result.store(&mut out[start..start + N]);
    }
    
    if remainder > 0 {
        let start = chunks * N;
        for i in 0..remainder {
            out[start + i] = a[start + i] + scalar;
        }
    }
}

// -----------------------------------------------------------------------------
// Тесты
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    
    // Тестовые реализации для скалярных векторов будут добавлены позже
    // #[test]
    // fn test_add_slices() {
    // }
}