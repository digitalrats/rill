//! Acoustic sensors — hear sound

use crate::core::{SignalOrigin, WorldSignal, WorldTime};
use crate::sensor::Sensor;
use rill_core::queues::Telemetry;
use std::collections::VecDeque;

/// Trait for audio analysis algorithms
pub trait Hearing: Send + 'static {
    /// Process a block of audio data
    fn process(&mut self, audio: &[f32]) -> f32;
    
    /// Name of the algorithm
    fn name(&self) -> &str;
}

/// Pitch detector
pub struct PitchDetector {
    sample_rate: f32,
    min_freq: f32,
    max_freq: f32,
    last_pitch: f32,
    buffer: VecDeque<f32>,
}

impl PitchDetector {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            min_freq: 20.0,
            max_freq: 2000.0,
            last_pitch: 0.0,
            buffer: VecDeque::with_capacity(2048),
        }
    }
    
    /// Autocorrelation for pitch detection
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
        // Add to buffer
        for &sample in audio {
            self.buffer.push_back(sample);
        }
        
        // Keep only the last 2048 samples
        while self.buffer.len() > 2048 {
            self.buffer.pop_front();
        }
        
        // Convert to vector for analysis
        let signal: Vec<f32> = self.buffer.iter().copied().collect();
        
        if let Some(pitch) = self.autocorrelate(&signal) {
            self.last_pitch = pitch;
        }
        
        // Normalize to 0-1
        (self.last_pitch - self.min_freq) / (self.max_freq - self.min_freq)
    }
    
    fn name(&self) -> &str {
        "pitch"
    }
}

/// Envelope follower (tracks amplitude)
pub struct EnvelopeFollower {
    attack: f32,
    release: f32,
    envelope: f32,
    sample_rate: f32,
}

impl EnvelopeFollower {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            attack: 0.01,
            release: 0.1,
            envelope: 0.0,
            sample_rate,
        }
    }
    
    pub fn with_attack(mut self, attack_sec: f32) -> Self {
        self.attack = attack_sec;
        self
    }
    
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

/// Zero-crossing detector
pub struct ZeroCrossing {
    last_sample: f32,
    crossings: u32,
    samples: u32,
    sample_rate: f32,
    frequency: f32,
}

impl ZeroCrossing {
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
        
        if self.samples > self.sample_rate as u32 / 10 { // Every 100ms
            self.frequency = self.crossings as f32 / (self.samples as f32 / self.sample_rate);
            self.crossings = 0;
            self.samples = 0;
        }
        
        self.frequency / 1000.0 // Normalization
    }
    
    fn name(&self) -> &str {
        "zero_crossing"
    }
}

/// Acoustic sensor
pub struct AcousticSensor {
    name: String,
    hearing: Box<dyn Hearing>,
    listen_to: Option<String>,  // Node ID in Graph
    last_value: f32,
    last_send: f32,
    threshold: f32,
}

impl AcousticSensor {
    pub fn new(name: impl Into<String>, hearing: Box<dyn Hearing>) -> Self {
        Self {
            name: name.into(),
            hearing,
            listen_to: None,
            last_value: 0.0,
            last_send: 0.0,
            threshold: 0.01,  // 1% hysteresis
        }
    }
    
    pub fn listening_to(mut self, node_id: impl Into<String>) -> Self {
        self.listen_to = Some(node_id.into());
        self
    }
    
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold.clamp(0.0, 0.1);
        self
    }
    
    /// Process telemetry from the Graph
    pub fn process_telemetry(&mut self, telemetry: &Telemetry) -> Option<WorldSignal> {
        match telemetry {
            Telemetry::SignalData { node_id, data, .. } => {
                if Some(node_id.to_string()) == self.listen_to {
                    let value = self.hearing.process(data);
                    self.last_value = value;
                    
                    // Only send on significant change
                    if (value - self.last_send).abs() > self.threshold {
                        self.last_send = value;
                        
                        return Some(WorldSignal::new(
                            SignalOrigin::Sensor(self.name.clone()),
                            crate::core::SignalValue::continuous(value),
                        ));
                    }
                }
            }
            _ => {}
        }
        None
    }
}

impl Sensor for AcousticSensor {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn sense(&mut self, perception: &crate::world::Perception) -> Option<WorldSignal> {
        if let Some(node_id) = &self.listen_to {
            if let Some(audio) = perception.hear(node_id) {
                let value = self.hearing.process(audio);
                self.last_value = value;
                
                if (value - self.last_send).abs() > self.threshold {
                    self.last_send = value;
                    
                    return Some(WorldSignal::new(
                        SignalOrigin::Sensor(self.name.clone()),
                        crate::core::SignalValue::continuous(value),
                    ));
                }
            }
        }
        None
    }
    
    fn last_value(&self) -> f32 {
        self.last_value
    }
}