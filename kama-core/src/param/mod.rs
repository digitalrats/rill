use serde::{Serialize, Deserialize};

/// Тип значения параметра
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    String(String),
    Choice(String),
}

/// Тип параметра
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamType {
    Float,
    Int,
    Bool,
    String,
    Choice,
}

/// Диапазон значений параметра
#[derive(Debug, Clone)]
pub struct ParamRange {
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub step: Option<f32>,
}

impl ParamRange {
    pub fn new() -> Self {
        Self {
            min: None,
            max: None,
            step: None,
        }
    }
    
    pub fn with_min(mut self, min: f32) -> Self {
        self.min = Some(min);
        self
    }
    
    pub fn with_max(mut self, max: f32) -> Self {
        self.max = Some(max);
        self
    }
    
    pub fn with_step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }
}

/// Дескриптор параметра
#[derive(Debug, Clone)]
pub struct ParameterDescriptor {
    pub id: String,
    pub name: String,
    pub value: ParamValue,
    pub default: ParamValue,
    pub range: ParamRange,
    pub unit: Option<String>,
}
