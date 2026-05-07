//! DSP functions for lo-fi processing

pub mod dac_emulation;
pub mod filters;
pub mod noise;
pub mod quantization;
pub mod vintage;

// Re-exports for convenience
pub use dac_emulation::*;
pub use filters::*;
pub use noise::*;
pub use quantization::*;
pub use vintage::*;
