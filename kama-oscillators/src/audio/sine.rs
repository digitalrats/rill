//! Sine wave oscillator using kama-core-dsp with AudioNum

use kama_core::traits::processor::{Processor, ProcessResult};
use kama_core::traits::{ParameterId, ParamValue};
use kama_core_dsp::generators::basic::{BasicOscillator, Waveform};
use kama_core_dsp::math::AudioNum;
use kama_core_dsp::algorithm::Algorithm;
use std::marker::PhantomData;

/// Sine wave oscillator generic over floating point type
///
/// Uses the optimized BasicOscillator from kama-core-dsp with AudioNum trait.
/// Supports both f32 and f64 through type parameter T.
///
/// # Type Parameters
/// - `T`: Floating point type (f32 or f64) implementing AudioNum
/// - `BUF_SIZE`: Fixed block size for processing
///
/// # Parameters
/// - `frequency`: Base frequency in Hz
/// - `amplitude`: Output amplitude (0.0 to 1.0)
/// - `phase`: Initial phase offset (0.0 to 1.0)
/// - `fm_amount`: Frequency modulation amount
///
/// # Inputs
/// - Port 0: Optional frequency modulation input (normalized 0.0 to 1.0)
///
/// # Outputs
/// - Port 0: Sine wave output
pub struct SineOsc<T: AudioNum, const BUF_SIZE: usize> {
    /// Core DSP oscillator
    osc: BasicOscillator<T>,
    
    /// Base frequency in Hz
    frequency: T,
    
    /// Output amplitude (0.0 to 1.0)
    amplitude: T,
    
    /// Phase offset (0.0 to 1.0)
    phase_offset: T,
    
    /// FM amount in Hz (max deviation)
    fm_amount: T,
    
    /// Whether FM is enabled
    use_fm: bool,
    
    /// Sample rate
    sample_rate: T,
    
    /// Phantom data to satisfy const generic
    _phantom: PhantomData<[T; BUF_SIZE]>,
}

impl<T: AudioNum, const BUF_SIZE: usize> SineOsc<T, BUF_SIZE> {
    /// Create new sine oscillator with default settings
    pub fn new() -> Self {
        let sample_rate = T::from_f32(44100.0);
        let mut osc = BasicOscillator::new(
            Waveform::Sine,
            T::from_f32(440.0),
            T::from_f32(1.0)
        );
        
        Self {
            osc,
            frequency: T::from_f32(440.0),
            amplitude: T::from_f32(0.5),
            phase_offset: T::ZERO,
            fm_amount: T::ZERO,
            use_fm: false,
            sample_rate,
            _phantom: PhantomData,
        }
    }

    /// Set base frequency
    pub fn with_frequency(mut self, freq: T) -> Self {
        self.frequency = freq.max(T::from_f32(0.1)).min(T::from_f32(20000.0));
        self.osc.set_frequency(self.frequency.to_f32());
        self
    }

    /// Set output amplitude (0.0 to 1.0)
    pub fn with_amplitude(mut self, amp: T) -> Self {
        self.amplitude = amp.clamp(T::ZERO, T::from_f32(1.0));
        self
    }

    /// Set phase offset (0.0 to 1.0)
    pub fn with_phase(mut self, phase: T) -> Self {
        self.phase_offset = phase.clamp(T::ZERO, T::from_f32(1.0));
        self.osc.set_phase(self.phase_offset);
        self
    }

    /// Enable frequency modulation with specified amount
    pub fn with_fm(mut self, amount: T) -> Self {
        self.use_fm = true;
        self.fm_amount = amount.clamp(T::ZERO, T::from_f32(10.0));
        self
    }

    /// Convert ParamValue to T
    fn param_to_t(value: ParamValue) -> Option<T> {
        match value {
            ParamValue::Float(f) => Some(T::from_f32(f)),
            ParamValue::Int(i) => Some(T::from_f32(i as f32)),
            _ => None,
        }
    }

    /// Convert T to ParamValue
    fn t_to_param(value: T) -> ParamValue {
        ParamValue::Float(value.to_f32())
    }

    /// Generate a block of samples with FM
    fn generate_block_with_fm(&mut self, output: &mut [T; BUF_SIZE], fm_input: &[T; BUF_SIZE]) {
        let one = T::from_f32(1.0);
        let two = T::from_f32(2.0);
        let min_freq = T::from_f32(0.1);
        let max_freq = T::from_f32(20000.0);
        
        for i in 0..BUF_SIZE {
            // Calculate modulated frequency: fm_input is 0..1, convert to -1..1
            let mod_normalized = fm_input[i] * two - one;
            let modulation = mod_normalized * self.fm_amount;
            let modulated_freq = (self.frequency + modulation)
                .max(min_freq)
                .min(max_freq);
            
            // Update oscillator frequency for this sample
            self.osc.set_frequency(modulated_freq.to_f32());
            
            // Generate sample
            output[i] = T::from_f32(self.osc.process_sample(T::ZERO).to_f32()) * self.amplitude;
        }
        
        // Restore base frequency
        self.osc.set_frequency(self.frequency.to_f32());
    }

    /// Generate a block of samples without FM
    fn generate_block_no_fm(&mut self, output: &mut [T; BUF_SIZE]) {
        self.osc.set_frequency(self.frequency.to_f32());
        
        for i in 0..BUF_SIZE {
            output[i] = T::from_f32(self.osc.process_sample(T::ZERO).to_f32()) * self.amplitude;
        }
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> Default for SineOsc<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> Processor<BUF_SIZE> for SineOsc<T, BUF_SIZE> {
    type Sample = T;

    fn process(
        &mut self,
        inputs: &[&[T; BUF_SIZE]],
        outputs: &mut [&mut [T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        if outputs.is_empty() {
            return Ok(());
        }

        if self.use_fm && !inputs.is_empty() {
            self.generate_block_with_fm(outputs[0], inputs[0]);
        } else {
            self.generate_block_no_fm(outputs[0]);
        }

        Ok(())
    }

    fn num_inputs(&self) -> usize {
        if self.use_fm { 1 } else { 0 }
    }

    fn num_outputs(&self) -> usize {
        1
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "frequency" => Some(Self::t_to_param(self.frequency)),
            "amplitude" => Some(Self::t_to_param(self.amplitude)),
            "phase" => Some(Self::t_to_param(self.phase_offset)),
            "fm_amount" => Some(Self::t_to_param(self.fm_amount)),
            "use_fm" => Some(ParamValue::Bool(self.use_fm)),
            _ => None,
        }
    }

    fn set_parameter(&mut self, id: &ParameterId, value: ParamValue) -> ProcessResult<()> {
        match id.as_str() {
            "frequency" => {
                if let Some(f) = Self::param_to_t(value) {
                    self.frequency = f.max(T::from_f32(0.1)).min(T::from_f32(20000.0));
                    self.osc.set_frequency(self.frequency.to_f32());
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "amplitude" => {
                if let Some(a) = Self::param_to_t(value) {
                    self.amplitude = a.clamp(T::ZERO, T::from_f32(1.0));
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "phase" => {
                if let Some(p) = Self::param_to_t(value) {
                    self.phase_offset = p.clamp(T::ZERO, T::from_f32(1.0));
                    self.osc.set_phase(self.phase_offset);
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "fm_amount" => {
                if let Some(a) = Self::param_to_t(value) {
                    self.fm_amount = a.clamp(T::ZERO, T::from_f32(10.0));
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected float".into()))
                }
            }
            "use_fm" => {
                if let ParamValue::Bool(b) = value {
                    self.use_fm = b;
                    Ok(())
                } else {
                    Err(ProcessError::Parameter("Expected bool".into()))
                }
            }
            _ => Err(ProcessError::Parameter(format!("Unknown parameter: {}", id))),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = T::from_f32(sample_rate);
        self.osc.init(sample_rate);
        self.osc.set_frequency(self.frequency.to_f32());
        self.osc.set_phase(self.phase_offset);
    }

    fn reset(&mut self) {
        self.osc.reset();
        self.osc.set_phase(self.phase_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;

    #[test]
    fn test_sine_creation_f32() {
        let osc = SineOsc::<f32, 64>::new()
            .with_frequency(440.0)
            .with_amplitude(0.7)
            .with_phase(0.25);
        
        assert!(approx_eq!(f32, osc.frequency, 440.0));
        assert!(approx_eq!(f32, osc.amplitude, 0.7));
        assert!(approx_eq!(f32, osc.phase_offset, 0.25));
    }

    #[test]
    fn test_sine_creation_f64() {
        let osc = SineOsc::<f64, 64>::new()
            .with_frequency(440.0)
            .with_amplitude(0.7)
            .with_phase(0.25);
        
        assert!((osc.frequency - 440.0).abs() < 1e-10);
        assert!((osc.amplitude - 0.7).abs() < 1e-10);
        assert!((osc.phase_offset - 0.25).abs() < 1e-10);
    }

    #[test]
    fn test_sine_generation_f32() {
        let mut osc = SineOsc::<f32, 64>::new()
            .with_frequency(440.0)
            .with_amplitude(0.5);
        
        osc.init(44100.0);
        
        let mut output = [0.0; 64];
        let mut outputs = [&mut output];
        
        osc.process(&[], &mut outputs).unwrap();
        
        // First sample should be near 0 (sine with phase 0)
        assert!(approx_eq!(f32, output[0], 0.0, epsilon = 1e-4));
        
        // All samples should be within amplitude range
        for &sample in &output {
            assert!(sample >= -0.5 && sample <= 0.5);
        }
    }

    #[test]
    fn test_sine_generation_f64() {
        let mut osc = SineOsc::<f64, 64>::new()
            .with_frequency(440.0)
            .with_amplitude(0.5);
        
        osc.init(44100.0);
        
        let mut output = [0.0; 64];
        let mut outputs = [&mut output];
        
        osc.process(&[], &mut outputs).unwrap();
        
        // First sample should be near 0
        assert!((output[0]).abs() < 1e-10);
        
        // All samples should be within amplitude range
        for &sample in &output {
            assert!(sample >= -0.5 && sample <= 0.5);
        }
    }

    #[test]
    fn test_sine_with_fm() {
        let mut osc = SineOsc::<f32, 64>::new()
            .with_frequency(440.0)
            .with_fm(2.0);
        
        osc.init(44100.0);
        
        let mut output = [0.0; 64];
        let fm_input = [0.5; 64];
        let inputs = [&fm_input];
        let mut outputs = [&mut output];
        
        osc.process(&inputs, &mut outputs).unwrap();
        
        // Should produce valid output
        assert!(output.iter().any(|&x| x != 0.0));
    }

    #[test]
    fn test_parameter_handling() {
        let mut osc = SineOsc::<f32, 64>::new();
        
        let freq_id = ParameterId::new("frequency").unwrap();
        osc.set_parameter(&freq_id, ParamValue::Float(880.0)).unwrap();
        assert!(approx_eq!(f32, osc.frequency, 880.0));
        
        let amp_id = ParameterId::new("amplitude").unwrap();
        osc.set_parameter(&amp_id, ParamValue::Float(0.3)).unwrap();
        assert!(approx_eq!(f32, osc.amplitude, 0.3));
        
        let phase_id = ParameterId::new("phase").unwrap();
        osc.set_parameter(&phase_id, ParamValue::Float(0.75)).unwrap();
        assert!(approx_eq!(f32, osc.phase_offset, 0.75));
    }
}