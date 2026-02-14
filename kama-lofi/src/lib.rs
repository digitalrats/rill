//! Lo-fi audio emulation: 8-bit, 12-bit, and classic digital audio systems

#![warn(missing_docs)]

mod error;
mod config;
mod dsp;
mod lofi_processor;
pub mod emulators;
pub mod utils;

// Реэкспорт основных типов
pub use error::{LofiError, LofiResult};
pub use config::{ClassicSystem, HardwareEmulation, LofiConfig};
pub use lofi_processor::LofiProcessor;
pub use emulators::{NesEmulator, Ay38910Emulator, AkaiS900Emulator};

// Реэкспорт утилит
pub use utils::*;

// Реэкспорт для удобства
pub use kama_core::AudioNode;

#[cfg(feature = "buffers")]
pub mod buffer_integration;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_quantization() {
        let test_signal = vec![0.1, 0.5, 0.9, -0.3, -0.8];
        
        let quantized_8bit: Vec<f32> = test_signal.iter()
            .map(|&s| dsp::quantize(s, 8, false))
            .collect();
        
        let quantized_12bit: Vec<f32> = test_signal.iter()
            .map(|&s| dsp::quantize(s, 12, false))
            .collect();
        
        let error_8bit: f32 = test_signal.iter()
            .zip(quantized_8bit.iter())
            .map(|(&a, &b)| (a - b).abs())
            .sum();
            
        let error_12bit: f32 = test_signal.iter()
            .zip(quantized_12bit.iter())
            .map(|(&a, &b)| (a - b).abs())
            .sum();
        
        assert!(error_12bit < error_8bit);
    }
    
    #[test]
    fn test_lofi_processor() {
        let config = LofiConfig {
            system: ClassicSystem::Custom {
                bit_depth: 8,
                sample_rate: 22050.0,
                nonlinear: false,
                noise_floor: -48.0,
            },
            ..Default::default()
        };
        
        let mut processor = LofiProcessor::new(config);
        processor.init(44100.0);
        
        let input = vec![0.5f32; 1024];
        let mut output = vec![0.0f32; 1024];
        
        let inputs = [&input[..]];
        let mut outputs = [&mut output[..]];
        
        processor.process(&inputs, &mut outputs).unwrap();
        
        assert_ne!(input[0], output[0]);
        assert!(output.iter().all(|&x| x.abs() <= 1.0));
    }
    
    #[test]
    fn test_nes_emulator() {
        let mut nes = NesEmulator::new(44100.0);
        
        let mut output = vec![0.0f32; 1024];
        let mut outputs = [&mut output[..]];
        
        nes.process(&[], &mut outputs).unwrap();
        
        assert!(output.iter().any(|&x| x != 0.0));
    }
    
    #[test]
    fn test_ay38910_basic() {
        let mut ay = Ay38910Emulator::new(44100.0);
        
        ay.write_register(0, 0x00);
        ay.write_register(1, 0x01);
        ay.write_register(8, 0x0F);
        ay.write_register(7, 0x3E);
        
        let mut output = vec![0.0f32; 1024];
        let mut outputs = [&mut output[..]];
        
        ay.process(&[], &mut outputs).unwrap();
        
        assert!(output.iter().any(|&x| x != 0.0));
        
        for &sample in &output {
            assert!(sample >= -1.0 && sample <= 1.0);
        }
    }
}