//! Physical sensors — sense touch (knobs, buttons)

use crate::core::{SignalOrigin, SignalValue, WorldSignal};
use crate::sensor::Sensor;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Physical sensor
pub struct PhysicalSensor {
    name: String,
    sensor_type: PhysicalType,
    physical_value: Arc<AtomicU32>,  // 0-65535
    last_sent: Arc<AtomicU32>,
    threshold: u32,
}

/// Type of physical sensor
pub enum PhysicalType {
    /// Rotary knob
    Knob {
        min: f32,
        max: f32,
        curve: KnobCurve,
    },
    /// Button (momentary)
    Button,
    /// Switch (discrete positions)
    Switch {
        positions: Vec<String>,
    },
}

/// Knob response curve
pub enum KnobCurve {
    Linear,
    Logarithmic,
    Exponential,
}

impl PhysicalSensor {
    /// Create a knob
    pub fn knob(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sensor_type: PhysicalType::Knob {
                min: 0.0,
                max: 1.0,
                curve: KnobCurve::Linear,
            },
            physical_value: Arc::new(AtomicU32::new(0)),
            last_sent: Arc::new(AtomicU32::new(0)),
            threshold: 64,  // ~0.1% hysteresis
        }
    }
    
    /// Create a button
    pub fn button(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sensor_type: PhysicalType::Button,
            physical_value: Arc::new(AtomicU32::new(0)),
            last_sent: Arc::new(AtomicU32::new(0)),
            threshold: 1,
        }
    }
    
    /// Set range for knob
    pub fn with_range(mut self, min: f32, max: f32) -> Self {
        if let PhysicalType::Knob { ref mut min: m, ref mut max: mx, .. } = self.sensor_type {
            *m = min;
            *mx = max;
        }
        self
    }
    
    /// Set curve
    pub fn with_curve(mut self, curve: KnobCurve) -> Self {
        if let PhysicalType::Knob { ref mut curve: c, .. } = self.sensor_type {
            *c = curve;
        }
        self
    }
    
    /// Set physical value (0-65535)
    pub fn set_physical(&self, value: u32) {
        self.physical_value.store(value & 0xFFFF, Ordering::Relaxed);
    }
    
    /// Press the button
    pub fn press(&self) {
        if let PhysicalType::Button = self.sensor_type {
            self.physical_value.store(1, Ordering::Relaxed);
        }
    }
    
    /// Release the button
    pub fn release(&self) {
        if let PhysicalType::Button = self.sensor_type {
            self.physical_value.store(0, Ordering::Relaxed);
        }
    }
    
    /// Convert physical value to normalized
    fn normalize(&self, phys: u32) -> f32 {
        let norm = phys as f32 / 65535.0;
        
        match &self.sensor_type {
            PhysicalType::Knob { curve, .. } => match curve {
                KnobCurve::Linear => norm,
                KnobCurve::Logarithmic => {
                    if norm <= 0.0 { 0.0 } else { (1.0 + norm * 9.0).log10() }
                }
                KnobCurve::Exponential => norm * norm,
            },
            PhysicalType::Button => {
                if phys > 0 { 1.0 } else { 0.0 }
            }
            PhysicalType::Switch { positions } => {
                let idx = (norm * (positions.len() - 1) as f32).round() as usize;
                idx as f32 / (positions.len() - 1) as f32
            }
        }
    }
}

impl Sensor for PhysicalSensor {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn sense(&mut self, _perception: &crate::world::Perception) -> Option<WorldSignal> {
        let current = self.physical_value.load(Ordering::Relaxed);
        let last = self.last_sent.load(Ordering::Relaxed);
        
        if (current as i32 - last as i32).abs() > self.threshold as i32 {
            self.last_sent.store(current, Ordering::Relaxed);
            
            let value = self.normalize(current);
            
            Some(WorldSignal::new(
                SignalOrigin::Sensor(self.name.clone()),
                SignalValue::continuous(value),
            ))
        } else {
            None
        }
    }
    
    fn last_value(&self) -> f32 {
        let current = self.physical_value.load(Ordering::Relaxed);
        self.normalize(current)
    }
}