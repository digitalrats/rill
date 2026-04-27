//! Lo-fi audio emulation: 8-bit, 12-bit, and classic digital audio systems

//#![warn(missing_docs)]
#![allow(missing_docs)]

mod config;
mod error;
mod lofi_processor;
// Публичные модули
pub mod dsp;
pub mod emulators;

// Реэкспорт основных типов
pub use config::{ClassicSystem, HardwareEmulation, LofiConfig};
pub use emulators::{AkaiS900Emulator, Ay38910Emulator, NesEmulator};
pub use error::{LofiError, LofiResult};
pub use lofi_processor::LofiProcessor;

// Реэкспорт для удобства
pub use rill_core::traits::AudioNode;

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
