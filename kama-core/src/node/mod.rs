use std::collections::HashMap;
use parking_lot::RwLock;

// Re-export базовых типов из kama-core-traits
pub use kama_core_traits::node::{
    AudioNode,
    NodeCategory,
    NodeMetadata,
    NodeCreator,
    NodeTypeId,
};

pub use kama_core_traits::param::ParamMetadata;

use crate::param::ParamValue;
use crate::AudioError;

/// Фабрика узлов
pub struct NodeFactory {
    registry: HashMap<String, Box<dyn NodeCreator>>,
}

impl NodeFactory {
    pub fn new() -> Self {
        Self {
            registry: HashMap::new(),
        }
    }
    
    pub fn register(&mut self, name: &str, creator: Box<dyn NodeCreator>) {
        self.registry.insert(name.to_string(), creator);
    }
    
    pub fn create(&self, name: &str) -> Option<Box<dyn AudioNode>> {
        self.registry.get(name).and_then(|c| c.create())
    }
}

/// Реестр узлов
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
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<GainNode>()
    }

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
            "gain" => Some(ParamValue::Float(self.gain)),
            "smoothing" => Some(ParamValue::Float(self.smoothing)),
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
                    typ: crate::param::ParamType::Float,
                    default: ParamValue::Float(1.0),
                    min: Some(0.0),
                    max: Some(4.0),
                    step: Some(0.01),
                    unit: Some("linear".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "smoothing".to_string(),
                    typ: crate::param::ParamType::Float,
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
    
    fn metadata(&self) -> NodeMetadata {
        // Явно вызываем метод из AudioNode
        AudioNode::metadata(self)
    }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<GainNode>()
    }
}