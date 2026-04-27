//! DSP функции для lo-fi обработки

pub mod dac_emulation;
pub mod filters;
pub mod noise;
pub mod quantization;
pub mod vintage;

// Реэкспорты для удобства
pub use dac_emulation::*;
pub use filters::*;
pub use noise::*;
pub use quantization::*;
pub use vintage::*;
