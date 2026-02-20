//! SIMD оптимизации для DSP (заглушка)

/// Маркерный трейт для SIMD-совместимых типов
pub trait SimdCompatible {}

impl SimdCompatible for f32 {}
impl SimdCompatible for f64 {}

/// SIMD-оптимизированная версия контекста
#[cfg(feature = "simd")]
pub struct SimdDspContext {
    // TODO: реализовать
}