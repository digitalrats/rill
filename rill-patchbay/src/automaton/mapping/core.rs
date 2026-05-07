//! Core types for mapping

use rill_core::traits::{ParameterId, PortId};
use std::sync::Arc;

/// Value transform type
#[derive(Debug, Clone)]
pub enum Transform {
    /// Linear: y = x
    Linear,
    
    /// Exponential: y = x²
    Exponential,
    
    /// Logarithmic: y = log10(1 + 9x)
    Logarithmic,
    
    /// Inverted: y = 1 - x
    Inverted,
    
    /// Scale: y = x * scale + offset
    Scale { scale: f32, offset: f32 },
    
    /// Threshold: y = 1 if x > threshold else 0
    Threshold { level: f32, hysteresis: f32 },
    
    /// Smoothing (exponential)
    Smooth { coefficient: f32 },
    
    /// RMS (for audio)
    Rms { window_size: usize },
    
    /// Peak detector
    Peak { decay: f32 },
    
    /// Envelope follower
    
    /// Frequency (zero-crossing)
    Frequency { min_freq: f32, max_freq: f32 },
    
    /// Custom function
    Custom(Arc<dyn Fn(f32) -> f32 + Send + Sync>),
}

impl Transform {
    /// Apply transform to a normalized value (0-1)
    pub fn apply(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        
        match self {
            Transform::Linear => x,
            Transform::Exponential => x * x,
            Transform::Logarithmic => {
                if x <= 0.0 { 0.0 } else { (1.0 + 9.0 * x).log10() }
            }
            Transform::Inverted => 1.0 - x,
            Transform::Scale { scale, offset } => x * scale + offset,
            Transform::Threshold { level, hysteresis } => {
                static mut STATE: bool = false;
                unsafe {
                    if x > *level + hysteresis {
                        STATE = true;
                        1.0
                    } else if x < *level - hysteresis {
                        STATE = false;
                        0.0
                    } else if STATE {
                        1.0
                    } else {
                        0.0
                    }
                }
            }
            Transform::Smooth { coefficient } => {
                static mut LAST: f32 = 0.0;
                unsafe {
                    LAST = LAST * (1.0 - coefficient) + x * coefficient;
                    LAST
                }
            }
            _ => x, // Placeholder for others for now
        }
    }
}

/// Mapping rule
#[derive(Debug, Clone)]
pub struct MappingRule {
    /// Input signal name (for automaton world)
    pub input_name: String,
    
    /// Input channel index (for audio graph)
    pub input_channel: usize,
    
    /// Transform
    pub transform: Transform,
    
    /// Output signal name (for automaton world)
    pub output_name: String,
    
    /// Target port (for direct control in audio graph)
    pub target_port: Option<PortId>,
    
    /// Target parameter
    pub target_parameter: Option<ParameterId>,
    
    /// Output value range
    pub output_range: (f32, f32),
}

impl MappingRule {
    /// Create a new rule
    pub fn new(input_name: impl Into<String>, output_name: impl Into<String>) -> Self {
        Self {
            input_name: input_name.into(),
            input_channel: 0,
            transform: Transform::Linear,
            output_name: output_name.into(),
            target_port: None,
            target_parameter: None,
            output_range: (0.0, 1.0),
        }
    }
    
    /// Set the transform
    pub fn with_transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }
    
    /// Set output range
    pub fn with_range(mut self, min: f32, max: f32) -> Self {
        self.output_range = (min, max);
        self
    }
    
    /// Set input channel (for audio)
    pub fn with_channel(mut self, channel: usize) -> Self {
        self.input_channel = channel;
        self
    }
    
    /// Set target parameter (for micro-control)
    pub fn with_target(mut self, port: PortId, parameter: ParameterId) -> Self {
        self.target_port = Some(port);
        self.target_parameter = Some(parameter);
        self
    }
    
    /// Apply rule to the value
    pub fn apply(&self, x: f32) -> f32 {
        let transformed = self.transform.apply(x);
        self.output_range.0 + transformed * (self.output_range.1 - self.output_range.0)
    }
}