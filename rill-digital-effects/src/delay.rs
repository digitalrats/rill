//! Delay effect with feedback

use rill_core::{
    buffer::DelayLine, math::Transcendental, traits::algorithm::Algorithm,
    traits::algorithm::AlgorithmCategory, traits::algorithm::AlgorithmMetadata,
    traits::ProcessResult,
};

/// Maximum delay time in seconds
const MAX_DELAY_SECONDS: f32 = 0.5;
/// Maximum sample rate we support (48 kHz)
const MAX_SAMPLE_RATE: f32 = 48_000.0;
/// Maximum delay in samples (2 seconds at max sample rate)
const MAX_DELAY_SAMPLES: usize = (MAX_DELAY_SECONDS * MAX_SAMPLE_RATE) as usize;

/// Delay effect with feedback
///
/// Parameters:
/// - delay_time: delay time in seconds (0.01 - 2.0)
/// - feedback: feedback amount (0.0 - 0.99)
/// - mix: dry/wet mix (0.0 - 1.0)
pub struct Delay<T: Transcendental, const BUF_SIZE: usize> {
    delay_time: f32,
    delay_samples: usize,
    feedback: f32,
    mix: f32,
    delay_line: DelayLine<T, MAX_DELAY_SAMPLES>,
    sample_rate: f32,
}

impl<T: Transcendental, const BUF_SIZE: usize> Delay<T, BUF_SIZE> {
    pub fn new(sample_rate: f32) -> Self {
        let delay_time = 0.5;
        let delay_samples = (delay_time * sample_rate) as usize;
        let mut delay_line = DelayLine::new(sample_rate);
        delay_line.set_delay_samples(delay_samples);

        Self {
            delay_time,
            delay_samples,
            feedback: 0.3,
            mix: 0.5,
            delay_line,
            sample_rate,
        }
    }

    pub fn with_params(sample_rate: f32, delay_time: f32, feedback: f32, mix: f32) -> Self {
        let mut instance = Self::new(sample_rate);
        instance.set_delay_time(delay_time);
        instance.set_feedback(feedback);
        instance.set_mix(mix);
        instance
    }

    pub fn set_delay_time(&mut self, time: f32) {
        self.delay_time = time.clamp(0.01, MAX_DELAY_SECONDS);
        self.update_delay_samples();
    }

    pub fn set_feedback(&mut self, fb: f32) {
        self.feedback = fb.clamp(0.0, 0.99);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    fn update_delay_samples(&mut self) {
        self.delay_samples = (self.delay_time * self.sample_rate) as usize;
        if self.delay_samples >= MAX_DELAY_SAMPLES {
            self.delay_samples = MAX_DELAY_SAMPLES - 1;
        }
        self.delay_line.set_delay_samples(self.delay_samples);
    }

    pub fn process_sample(&mut self, input: T) -> T {
        let delayed = self.delay_line.read_delayed(self.delay_samples);
        let dry = input;
        let wet = delayed;
        let mix = T::from_f32(self.mix);
        let one_minus_mix = T::ONE - mix;
        let output = dry.mul(one_minus_mix).add(wet.mul(mix));
        let feedback = T::from_f32(self.feedback);
        let write_sample = input.add(delayed.mul(feedback));
        self.delay_line.write(write_sample);
        output
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for Delay<T, BUF_SIZE> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.delay_line = DelayLine::new(sample_rate);
        self.update_delay_samples();
    }

    fn reset(&mut self) {
        self.delay_line = DelayLine::new(self.sample_rate);
        self.update_delay_samples();
    }

    fn process(&mut self, input: Option<&[T]>, output: &mut [T]) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        for i in 0..len {
            output[i] = self.process_sample(input[i]);
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: "Delay",
            category: AlgorithmCategory::Effect,
            description: "Delay effect with feedback control",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}
