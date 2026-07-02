//! Lo-fi audio emulation: 8-bit, 12-bit, and classic digital audio systems

#![warn(missing_docs)]

mod chip_emulator;
mod config;
/// Digital signal processing utilities (quantization, bitcrushing, etc.).
pub mod dsp;
/// Hardware emulators (NES, AY-3-8910, Akai S900).
pub mod emulators;
mod error;
mod lofi_chip_source;
mod lofi_processor;

// Re-export core types
pub use chip_emulator::ChipEmulator;
pub use config::{ClassicSystem, HardwareEmulation, LofiConfig};
pub use emulators::{AkaiS900Emulator, Ay38910Chip, NesChip};
pub use error::{LofiError, LofiResult};
pub use lofi_chip_source::LofiChipSource;
pub use lofi_processor::LofiProcessor;

// Re-export for convenience
pub use rill_core::traits::Node;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quantization() {
        let test_signal: Vec<f32> = vec![0.1, 0.5, 0.9, -0.3, -0.8];

        let quantized_8bit: Vec<f32> = test_signal
            .iter()
            .map(|&s| dsp::quantization::bitcrush(s, 8, false))
            .collect();

        let quantized_12bit: Vec<f32> = test_signal
            .iter()
            .map(|&s| dsp::quantization::bitcrush(s, 12, false))
            .collect();

        let error_8bit: f32 = test_signal
            .iter()
            .zip(quantized_8bit.iter())
            .map(|(&a, &b)| (a - b).abs())
            .sum();

        let error_12bit: f32 = test_signal
            .iter()
            .zip(quantized_12bit.iter())
            .map(|(&a, &b)| (a - b).abs())
            .sum();

        assert!(error_12bit < error_8bit);
    }
}
