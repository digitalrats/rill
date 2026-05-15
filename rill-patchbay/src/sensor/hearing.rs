//! # Hearing — signal analysis for acoustic sensors
//!
//! Algorithms that analyse signal buffers and produce scalar features
//! (pitch, envelope, zero-crossing rate). Used by [`AcousticSensor`]
//! to turn signal data into control parameters.
//!
//! Future: wire these into graph telemetry so `AcousticSensor` receives
//! `Telemetry::SignalData` from a specific graph node.

use std::collections::VecDeque;

/// Trait for signal analysis algorithms.
pub trait Hearing: Send + 'static {
    /// Process a block of signal data and return a scalar value.
    fn process(&mut self, audio: &[f32]) -> f32;

    /// Name of the algorithm.
    fn name(&self) -> &str;
}

/// Pitch detector using autocorrelation.
pub struct PitchDetector {
    sample_rate: f32,
    min_freq: f32,
    max_freq: f32,
    last_pitch: f32,
    buffer: VecDeque<f32>,
}

impl PitchDetector {
    /// Create a new pitch detector.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            min_freq: 20.0,
            max_freq: 2000.0,
            last_pitch: 0.0,
            buffer: VecDeque::with_capacity(2048),
        }
    }

    fn autocorrelate(&self, signal: &[f32]) -> Option<f32> {
        if signal.len() < 100 {
            return None;
        }
        let min_period = (self.sample_rate / self.max_freq) as usize;
        let max_period = (self.sample_rate / self.min_freq) as usize;
        let mut best_corr = 0.0;
        let mut best_period = min_period;

        for period in min_period..max_period.min(signal.len() / 2) {
            let mut corr = 0.0;
            let mut energy = 0.0;
            for i in 0..period {
                if i + period < signal.len() {
                    corr += signal[i] * signal[i + period];
                    energy += signal[i] * signal[i] + signal[i + period] * signal[i + period];
                }
            }
            if energy > 0.0 {
                let norm_corr = corr / (energy.sqrt() + 1e-6);
                if norm_corr > best_corr {
                    best_corr = norm_corr;
                    best_period = period;
                }
            }
        }

        if best_corr > 0.1 {
            Some(self.sample_rate / best_period as f32)
        } else {
            None
        }
    }
}

impl Hearing for PitchDetector {
    fn process(&mut self, audio: &[f32]) -> f32 {
        for &sample in audio {
            self.buffer.push_back(sample);
        }
        while self.buffer.len() > 2048 {
            self.buffer.pop_front();
        }
        let signal: Vec<f32> = self.buffer.iter().copied().collect();
        if let Some(pitch) = self.autocorrelate(&signal) {
            self.last_pitch = pitch;
        }
        (self.last_pitch - self.min_freq) / (self.max_freq - self.min_freq)
    }

    fn name(&self) -> &str {
        "pitch"
    }
}

/// Envelope follower (tracks amplitude).
pub struct EnvelopeFollower {
    attack: f32,
    release: f32,
    envelope: f32,
    sample_rate: f32,
}

impl EnvelopeFollower {
    /// Create a new envelope follower.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            attack: 0.01,
            release: 0.1,
            envelope: 0.0,
            sample_rate,
        }
    }

    /// Set attack time in seconds.
    pub fn with_attack(mut self, attack_sec: f32) -> Self {
        self.attack = attack_sec;
        self
    }

    /// Set release time in seconds.
    pub fn with_release(mut self, release_sec: f32) -> Self {
        self.release = release_sec;
        self
    }
}

impl Hearing for EnvelopeFollower {
    fn process(&mut self, audio: &[f32]) -> f32 {
        let attack_coef = (-1.0 / (self.attack * self.sample_rate)).exp();
        let release_coef = (-1.0 / (self.release * self.sample_rate)).exp();
        for &sample in audio {
            let input = sample.abs();
            if input > self.envelope {
                self.envelope = attack_coef * self.envelope + (1.0 - attack_coef) * input;
            } else {
                self.envelope = release_coef * self.envelope + (1.0 - release_coef) * input;
            }
        }
        self.envelope
    }

    fn name(&self) -> &str {
        "envelope"
    }
}

/// Zero-crossing frequency detector.
pub struct ZeroCrossing {
    last_sample: f32,
    crossings: u32,
    samples: u32,
    sample_rate: f32,
    frequency: f32,
}

impl ZeroCrossing {
    /// Create a new zero-crossing detector.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            last_sample: 0.0,
            crossings: 0,
            samples: 0,
            sample_rate,
            frequency: 0.0,
        }
    }
}

impl Hearing for ZeroCrossing {
    fn process(&mut self, audio: &[f32]) -> f32 {
        for &sample in audio {
            if self.last_sample <= 0.0 && sample > 0.0 {
                self.crossings += 1;
            }
            self.last_sample = sample;
            self.samples += 1;
        }
        if self.samples > self.sample_rate as u32 / 10 {
            self.frequency = self.crossings as f32 / (self.samples as f32 / self.sample_rate);
            self.crossings = 0;
            self.samples = 0;
        }
        self.frequency / 1000.0
    }

    fn name(&self) -> &str {
        "zero_crossing"
    }
}
