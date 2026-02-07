use std::any::Any;
use crate::param::{ParamValue, ParamType};
use crate::AudioError;

/// Базовый трейт для всех аудиоузлов
pub trait AudioNode: Send + Sync + Any {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError>;
    fn get_param(&self, name: &str) -> Option<ParamValue>;
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError>;
    fn init(&mut self, sample_rate: f32);
    fn reset(&mut self);
    fn num_inputs(&self) -> usize;
    fn num_outputs(&self) -> usize;
    fn metadata(&self) -> NodeMetadata;
}

/// Метаданные узла
#[derive(Clone)]
pub struct NodeMetadata {
    pub name: String,
    pub category: NodeCategory,
    pub description: String,
    pub author: String,
    pub version: String,
    pub parameters: Vec<ParamMetadata>,
}

/// Категории узлов
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeCategory {
    Generator,
    Effect,
    Filter,
    Mixer,
    Utility,
    Analyzer,
    Midi,
    Sequencer,
}

/// Метаданные параметра
#[derive(Clone)]
pub struct ParamMetadata {
    pub name: String,
    pub typ: ParamType,
    pub default: ParamValue,
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub step: Option<f32>,
    pub unit: Option<String>,
    pub choices: Option<Vec<(String, f32)>>,
}

/// Фабрика узлов
pub struct NodeFactory {
    registry: std::collections::HashMap<String, Box<dyn NodeCreator>>,
}

impl NodeFactory {
    pub fn new() -> Self {
        Self {
            registry: std::collections::HashMap::new(),
        }
    }
    
    pub fn register(&mut self, name: &str, creator: Box<dyn NodeCreator>) {
        self.registry.insert(name.to_string(), creator);
    }
    
    pub fn create(&self, name: &str) -> Option<Box<dyn AudioNode>> {
        self.registry.get(name).and_then(|c| c.create())
    }
}

pub trait NodeCreator: Send + Sync {
    fn create(&self) -> Option<Box<dyn AudioNode>>;
}

/// Пример узла - Gain
#[derive(Default)]
pub struct GainNode {
    gain: f32,
    sample_rate: f32,
    smoothing: f32,
    smooth_gain: f32,
}

impl GainNode {
    pub fn new(gain: f32) -> Self {
        Self {
            gain,
            sample_rate: 44100.0,
            smoothing: 0.01,
            smooth_gain: gain,
        }
    }
}

impl AudioNode for GainNode {
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        if inputs.is_empty() || outputs.is_empty() {
            return Ok(());
        }
        
        let coeff = 1.0 - (-1.0 / (self.sample_rate * self.smoothing)).exp();
        self.smooth_gain += coeff * (self.gain - self.smooth_gain);
        
        let input = inputs[0];
        let output = &mut outputs[0];
        
        for i in 0..input.len().min(output.len()) {
            output[i] = input[i] * self.smooth_gain;
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "gain" => Some(crate::param::ParamValue::Float(self.gain)),
            "smoothing" => Some(crate::param::ParamValue::Float(self.smoothing)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("gain", ParamValue::Float(g)) => {
                self.gain = g.max(0.0);
                Ok(())
            }
            ("smoothing", ParamValue::Float(s)) => {
                self.smoothing = s.max(0.0).min(1.0);
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
    }
    
    fn reset(&mut self) {
        self.smooth_gain = self.gain;
    }
    
    fn num_inputs(&self) -> usize { 1 }
    fn num_outputs(&self) -> usize { 1 }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: "Gain".to_string(),
            category: NodeCategory::Effect,
            description: "Simple gain/volume control".to_string(),
            author: "Kama Core".to_string(),
            version: "1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "gain".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(4.0),
                    step: Some(0.01),
                    unit: Some("linear".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "smoothing".to_string(),
                    typ: ParamType::Float,
                    default: ParamValue::Float(0.01),
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.001),
                    unit: Some("seconds".to_string()),
                    choices: None,
                },
            ],
        }
    }
}

impl NodeCreator for GainNode {
    fn create(&self) -> Option<Box<dyn AudioNode>> {
        Some(Box::new(Self::default()))
    }
}


pub struct NodeRegistry {
    factory: NodeFactory,
}

impl NodeRegistry {
    pub fn new() -> Self {
        Self {
            factory: NodeFactory::new(),
        }
    }
    
    pub fn register(&mut self, name: &str, creator: Box<dyn NodeCreator>) {
        self.factory.register(name, creator);
    }
    
    pub fn create(&self, name: &str) -> Option<Box<dyn AudioNode>> {
        self.factory.create(name)
    }
}