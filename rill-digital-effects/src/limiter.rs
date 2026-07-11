//! Limiter with lookahead using Delay + envelope detection

use crate::delay::Delay;
use rill_core::{
    buffer::DelayLine,
    math::Transcendental,
    traits::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata},
    traits::ProcessResult,
};

/// Maximum lookahead time in seconds (10 ms)
const MAX_LOOKAHEAD_TIME: f32 = 0.01;
const MAX_SAMPLE_RATE: f32 = 192_000.0;
const MAX_LOOKAHEAD_SAMPLES: usize = (MAX_LOOKAHEAD_TIME * MAX_SAMPLE_RATE) as usize;
const ANALYSIS_BUF_SIZE: usize = MAX_LOOKAHEAD_SAMPLES * 2;

/// Limiter with lookahead using Delay + envelope detection
pub struct Limiter<T: Transcendental, const BUF_SIZE: usize> {
    delay: Delay<T, BUF_SIZE>,
    analysis_buffer: DelayLine<T, ANALYSIS_BUF_SIZE>,
    threshold_db: f32,
    threshold_linear: T,
    output_gain: f32,
    attack: f32,
    release: f32,
    lookahead: f32,
    lookahead_samples: usize,
    current_gain: f32,
    attack_coeff: f32,
    release_coeff: f32,
    sample_rate: f32,
    position: usize,
    init_buffer: Vec<T>,
    initializing: bool,
    warming_up: bool,
}

impl<T: Transcendental, const BUF_SIZE: usize> Limiter<T, BUF_SIZE> {
    pub fn new(
        sample_rate: f32,
        threshold_db: f32,
        attack: f32,
        release: f32,
        output_gain: f32,
    ) -> Self {
        let threshold_db = threshold_db.clamp(-60.0, 0.0);
        let threshold_linear = T::from_f32(10.0_f32.powf(threshold_db / 20.0));

        let attack = attack.clamp(0.001, 0.1);
        let release = release.clamp(0.01, 1.0);

        let attack_coeff = (-1.0 / (attack * sample_rate)).exp();
        let release_coeff = (-1.0 / (release * sample_rate)).exp();

        let lookahead = 0.005;
        let lookahead_samples = (lookahead * sample_rate) as usize;

        let delay = Delay::with_params(sample_rate, lookahead, 0.0, 1.0);
        let analysis_buffer = DelayLine::new(sample_rate);
        let init_buffer = Vec::with_capacity(lookahead_samples);

        Self {
            delay,
            analysis_buffer,
            threshold_db,
            threshold_linear,
            output_gain: output_gain.clamp(0.0, 2.0),
            attack,
            release,
            lookahead,
            lookahead_samples,
            current_gain: 1.0,
            attack_coeff,
            release_coeff,
            sample_rate,
            position: 0,
            init_buffer,
            initializing: true,
            warming_up: false,
        }
    }

    pub fn process_sample(&mut self, input: T) -> T {
        self.position += 1;

        self.analysis_buffer.write(input);
        let delayed = self.delay.process_sample(input);

        if self.initializing {
            self.init_buffer.push(input);
            if self.position >= self.lookahead_samples {
                self.initializing = false;
                self.warming_up = true;
                self.delay.reset();
            }
            return input;
        }

        if self.warming_up && self.position < self.lookahead_samples * 2 {
            if self.position - self.lookahead_samples <= self.init_buffer.len() {
                let idx = self.position - self.lookahead_samples - 1;
                if idx < self.init_buffer.len() {
                    let sample = self.init_buffer[idx];
                    let _ = self.delay.process_sample(sample);
                }
            }
            if self.position >= self.lookahead_samples * 2 - 1 {
                self.warming_up = false;
            }
            return input;
        }

        let mut max_amp = T::ZERO;
        for offset in 0..self.lookahead_samples {
            let sample = self.analysis_buffer.read_delayed(offset);
            let abs_sample = sample.abs();
            if abs_sample > max_amp {
                max_amp = abs_sample;
            }
        }

        let target_gain = if max_amp > self.threshold_linear {
            self.threshold_linear.div(max_amp).to_f32()
        } else {
            1.0
        };

        if target_gain < self.current_gain {
            self.current_gain =
                self.current_gain * self.attack_coeff + target_gain * (1.0 - self.attack_coeff);
        } else {
            self.current_gain =
                self.current_gain * self.release_coeff + target_gain * (1.0 - self.release_coeff);
        }

        let output = delayed.mul(T::from_f32(self.current_gain * self.output_gain));
        output.clamp(T::from_f32(-2.0), T::from_f32(2.0))
    }

    pub fn process_block(&mut self, input: &[T], output: &mut [T]) {
        for i in 0..input.len().min(output.len()) {
            output[i] = self.process_sample(input[i]);
        }
    }

    pub fn current_gain(&self) -> f32 {
        self.current_gain
    }

    pub fn lookahead_samples(&self) -> usize {
        self.lookahead_samples
    }

    pub fn set_threshold(&mut self, db: f32) {
        self.threshold_db = db.clamp(-60.0, 0.0);
        self.threshold_linear = T::from_f32(10.0_f32.powf(self.threshold_db / 20.0));
    }

    pub fn set_attack(&mut self, attack: f32) {
        self.attack = attack.clamp(0.001, 0.1);
        self.attack_coeff = (-1.0 / (self.attack * self.sample_rate)).exp();
    }

    pub fn set_release(&mut self, release: f32) {
        self.release = release.clamp(0.01, 1.0);
        self.release_coeff = (-1.0 / (self.release * self.sample_rate)).exp();
    }

    pub fn set_lookahead(&mut self, lookahead: f32) {
        self.lookahead = lookahead.clamp(0.0, 0.01);
        self.lookahead_samples = (self.lookahead * self.sample_rate) as usize;
        self.delay.set_delay_time(lookahead);
        self.analysis_buffer.clear();
        self.current_gain = 1.0;
        self.position = 0;
        self.init_buffer.clear();
        self.initializing = true;
        self.warming_up = false;
    }

    pub fn reset(&mut self) {
        self.current_gain = 1.0;
        self.position = 0;
        self.init_buffer.clear();
        self.initializing = true;
        self.warming_up = false;
        self.delay.reset();
        self.analysis_buffer.clear();
    }

    pub fn force_ready(&mut self) {
        if self.initializing || self.warming_up {
            for _ in 0..self.lookahead_samples * 2 {
                let test_val = T::from_f32(0.1);
                self.analysis_buffer.write(test_val);
                let _ = self.delay.process_sample(test_val);
            }
            self.initializing = false;
            self.warming_up = false;
            self.position = self.lookahead_samples * 2;
        }
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Algorithm<T> for Limiter<T, BUF_SIZE> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.attack_coeff = (-1.0 / (self.attack * sample_rate)).exp();
        self.release_coeff = (-1.0 / (self.release * sample_rate)).exp();
        self.lookahead_samples = (self.lookahead * sample_rate) as usize;
        self.delay = Delay::with_params(sample_rate, self.lookahead, 0.0, 1.0);
        self.analysis_buffer = DelayLine::new(sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.current_gain = 1.0;
        self.position = 0;
        self.init_buffer.clear();
        self.initializing = true;
        self.warming_up = false;
        self.delay.reset();
        self.analysis_buffer.clear();
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
            name: "Limiter",
            category: AlgorithmCategory::Effect,
            description: "Lookahead limiter with attack/release envelope",
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}
