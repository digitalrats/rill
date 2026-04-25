//! DSP функции для lo-fi обработки

pub mod quantization;
pub mod noise;
pub mod dac_emulation;
pub mod filters;
pub mod vintage;

// Реэкспорты для удобства
pub use quantization::*;
pub use noise::*;
pub use dac_emulation::*;
pub use filters::*;
pub use vintage::*;