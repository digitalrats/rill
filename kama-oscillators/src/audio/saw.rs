//! Sawtooth wave oscillator using kama-core-dsp with AudioNum

use kama_core::traits::{Processor, ParameterId, ParamValue};
use kama_core::{ProcessResult, ProcessError};
use kama_core_dsp::generators::basic::{BasicOscillator, Waveform};
use kama_core::AudioNum;
use kama_core_dsp::algorithm::Algorithm;
use std::marker::PhantomData;

/// Sawtooth wave oscillator generic over floating point type
///
/// Uses the optimized BasicOscillator from kama-core-dsp with built-in
/// BLEP anti-aliasing.
pub struct SawOsc<T: AudioNum, const BUF_SIZE: usize> {
    /// Core DSP oscillator
    osc: BasicOscillator<T>,
    
    /// Base frequency
    frequency: T,
    
    /// Output amplitude
    amplitude: T,
    
    /// Sample rate
    sample_rate: T,
    
    /// Phantom data
    _phantom: PhantomData<[T; BUF_SIZE]>,
}

impl<T: AudioNum, const BUF_SIZE: usize> SawOsc<T, BUF_SIZE> {
    /// Create new sawtooth oscillator
    pub fn new() -> Self {
        let sample_rate = T::from_f32(44100.0);
        let osc = BasicOscillator::new(
            Waveform::Saw,
            T::from_f32(440.0),
            T::from_f32(1.0)
        );
        
        Self {
            osc,
            frequency: T::from_f32(440.0),
            amplitude: T::from_f32(0.5),
            sample_rate,
            _phantom: PhantomData,
        }
    }

    /// Set frequency
    pub fn with_frequency(mut self, freq: T) -> Self {
        self.frequency = freq.max(T::from_f32(0.1)).min(T::from_f32(20000.0));
        self.osc.set_frequency(self.frequency.to_f32());
        self
    }

    /// Set amplitude
    pub fn with_amplitude(mut self, amp: T) -> Self {
        self.amplitude = amp.clamp(T::ZERO, T::from_f32(1.0));
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

    /// Generate a block of samples
    fn generate_block(&mut self, output: &mut [T; BUF_SIZE]) {
        self.osc.set_frequency(self.frequency.to_f32());
        
        for i in 0..BUF_SIZE {
            output[i] = T::from_f32(self.osc.process_sample(T::ZERO).to_f32()) * self.amplitude;
        }
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> Default for SawOsc<T, BUF_SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: AudioNum, const BUF_SIZE: usize> Processor<BUF_SIZE> for SawOsc<T, BUF_SIZE> {
    type Sample = T;

    fn process(
        &mut self,
        _inputs: &[&[T; BUF_SIZE]],
        outputs: &mut [&mut [T; BUF_SIZE]],
    ) -> ProcessResult<()> {
        if outputs.is_empty() {
            return Ok(());
        }

        self.generate_block(outputs[0]);
        Ok(())
    }

    fn num_inputs(&self) -> usize {
        0
    }

    fn num_outputs(&self) -> usize {
        1
    }

    fn get_parameter(&self, id: &ParameterId) -> Option<ParamValue> {
        match id.as_str() {
            "frequency" => Some(Self::t_to_param(self.frequency)),
            "amplitude" => Some(Self::t_to_param(self.amplitude)),
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
            _ => Err(ProcessError::Parameter(format!("Unknown parameter: {}", id))),
        }
    }

    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = T::from_f32(sample_rate);
        self.osc.init(sample_rate);
        self.osc.set_frequency(self.frequency.to_f32());
    }

    fn reset(&mut self) {
        self.osc.reset();
    }
}