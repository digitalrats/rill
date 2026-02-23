#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Тип значения параметра
#[derive(Debug, Clone, PartialEq)] // <-- добавляем PartialEq
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    String(String),
    Choice(String),
}

/// Тип параметра
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ParamType {
    Float,
    Int,
    Bool,
    String,
    Choice,
}

/// Диапазон значений параметра
#[derive(Debug, Clone, PartialEq)] // <-- добавляем PartialEq
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
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

/// Метаданные параметра
#[derive(Debug, Clone, PartialEq)] // <-- добавляем PartialEq
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
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
