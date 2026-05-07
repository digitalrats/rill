//! Parameter handling for audio nodes
//!
//! Defines the fundamental building blocks of the signal graph:
//! - `Node`: Base trait for all nodes
//! - `Source`: Active generator (has no inputs)
//! - `Processor`: Passive processor (has inputs and outputs)
//! - `Sink`: Active consumer (has no outputs)

use super::error::{ParameterError, ParameterResult};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

// ============================================================================
// Parameter ID
// ============================================================================

/// Type-safe parameter identifier with validation
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParameterId {
    name: String,
}

impl ParameterId {
    /// Maximum length of a parameter name
    pub const MAX_LEN: usize = 64;

    /// Create a new ParameterId with validation
    ///
    /// # Rules
    /// - Not empty
    /// - Max length MAX_LEN
    /// - Starts with a letter (a-z, A-Z)
    /// - Contains only letters, digits, and underscores
    pub fn new(name: impl Into<String>) -> ParameterResult<Self> {
        let name = name.into();

        if name.is_empty() {
            return Err(ParameterError::Empty);
        }

        if name.len() > Self::MAX_LEN {
            return Err(ParameterError::TooLong { max: Self::MAX_LEN });
        }

        let first = name.chars().next().unwrap();
        if !first.is_ascii_alphabetic() {
            return Err(ParameterError::MustStartWithLetter);
        }

        for c in name.chars() {
            if !c.is_ascii_alphanumeric() && c != '_' {
                return Err(ParameterError::InvalidCharacter(c));
            }
        }

        Ok(Self { name })
    }

    /// Get the string representation
    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// Convert into a String
    pub fn into_string(self) -> String {
        self.name
    }
}

impl AsRef<str> for ParameterId {
    fn as_ref(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for ParameterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl FromStr for ParameterId {
    type Err = ParameterError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ParameterId::new(s)
    }
}

// ============================================================================
// Parameter Type
// ============================================================================

/// Type of parameter value
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamType {
    /// Floating point value
    Float,

    /// Integer value
    Int,

    /// Boolean value
    Bool,

    /// String value
    String,

    /// Choice from a list of options
    Choice,
}

impl ParamType {
    /// Get the name of the parameter type
    pub fn name(&self) -> &'static str {
        match self {
            Self::Float => "float",
            Self::Int => "int",
            Self::Bool => "bool",
            Self::String => "string",
            Self::Choice => "choice",
        }
    }
}

impl fmt::Display for ParamType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// Parameter Value
// ============================================================================

/// Parameter value (can be of different types)
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ParamValue {
    /// Floating point value
    Float(f32),

    /// Integer value
    Int(i32),

    /// Boolean value
    Bool(bool),

    /// String value
    String(String),

    /// Choice from a list of options
    Choice(String),
}

impl ParamValue {
    /// Get the type of this value
    pub fn param_type(&self) -> ParamType {
        match self {
            Self::Float(_) => ParamType::Float,
            Self::Int(_) => ParamType::Int,
            Self::Bool(_) => ParamType::Bool,
            Self::String(_) => ParamType::String,
            Self::Choice(_) => ParamType::Choice,
        }
    }

    /// Try to convert to f32
    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::Float(f) => Some(*f),
            Self::Int(i) => Some(*i as f32),
            Self::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// Try to convert to i32
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Float(f) => Some(*f as i32),
            Self::Int(i) => Some(*i),
            Self::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    /// Try to convert to bool
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            Self::Float(f) => Some(*f > 0.5),
            Self::Int(i) => Some(*i > 0),
            _ => None,
        }
    }

    /// Return the string value if this is a `String` or `Choice` variant.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) | Self::Choice(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

// ============================================================================
// Parameter Range
// ============================================================================

/// Range constraints for a parameter
#[derive(Debug, Clone, PartialEq)]
pub struct ParamRange {
    /// Minimum value (if applicable)
    pub min: Option<f32>,

    /// Maximum value (if applicable)
    pub max: Option<f32>,

    /// Step size (if applicable)
    pub step: Option<f32>,
}

impl ParamRange {
    /// Create a new empty range
    pub fn new() -> Self {
        Self {
            min: None,
            max: None,
            step: None,
        }
    }

    /// Set minimum value
    pub fn with_min(mut self, min: f32) -> Self {
        self.min = Some(min);
        self
    }

    /// Set maximum value
    pub fn with_max(mut self, max: f32) -> Self {
        self.max = Some(max);
        self
    }

    /// Set step size
    pub fn with_step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self
    }

    /// Check if value is within range
    pub fn contains(&self, value: f32) -> bool {
        if let Some(min) = self.min {
            if value < min {
                return false;
            }
        }
        if let Some(max) = self.max {
            if value > max {
                return false;
            }
        }
        true
    }

    /// Clamp value to range
    pub fn clamp(&self, value: f32) -> f32 {
        let mut value = value;
        if let Some(min) = self.min {
            value = value.max(min);
        }
        if let Some(max) = self.max {
            value = value.min(max);
        }
        value
    }
}

impl Default for ParamRange {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Parameter Metadata
// ============================================================================

/// Metadata about a parameter
#[derive(Debug, Clone, PartialEq)]
pub struct ParamMetadata {
    /// Parameter name (must be a valid ParameterId)
    pub name: String,

    /// Human-readable description
    pub description: String,

    /// Parameter type
    pub typ: ParamType,

    /// Default value
    pub default: ParamValue,

    /// Value range (if applicable)
    pub range: ParamRange,

    /// Unit of measurement (e.g., "Hz", "dB", "ms")
    pub unit: Option<String>,

    /// Possible choices (for Choice parameters)
    pub choices: Option<Vec<(String, f32)>>,
}

impl ParamMetadata {
    /// Create new parameter metadata
    pub fn new(name: impl Into<String>, typ: ParamType, default: ParamValue) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            typ,
            default,
            range: ParamRange::default(),
            unit: None,
            choices: None,
        }
    }

    /// Set description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set range
    pub fn with_range(mut self, min: f32, max: f32, step: f32) -> Self {
        self.range = ParamRange::new()
            .with_min(min)
            .with_max(max)
            .with_step(step);
        self
    }

    /// Set unit
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Set choices
    pub fn with_choices(mut self, choices: Vec<(String, f32)>) -> Self {
        self.choices = Some(choices);
        self
    }
}

// ============================================================================
// Params — bag of parameters for factory-based node construction
// ============================================================================

/// A flexible set of parameters passed to a node constructor.
///
/// Uses `HashMap<String, ParamValue>` so any node type can extract
/// whatever named parameters it supports. This is intentionally
/// open-ended — no fixed schema, no required fields.
///
/// See `NodeConstructor` (in `rill-graph`) for how builder uses this.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Params {
    /// Sample rate the node will be initialized with.
    pub sample_rate: f32,

    /// Arbitrary named parameters.
    pub parameters: HashMap<String, ParamValue>,
}

impl Params {
    /// Create params with a given sample rate.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            parameters: HashMap::new(),
        }
    }

    /// Builder-style: insert a parameter and return self.
    pub fn with(mut self, key: impl Into<String>, value: ParamValue) -> Self {
        self.parameters.insert(key.into(), value);
        self
    }

    /// Get a parameter by name.
    pub fn get(&self, key: &str) -> Option<&ParamValue> {
        self.parameters.get(key)
    }

    /// Insert or overwrite a parameter.
    pub fn insert(&mut self, key: impl Into<String>, value: ParamValue) -> Option<ParamValue> {
        self.parameters.insert(key.into(), value)
    }

    /// Remove a parameter, returning its value if present.
    pub fn remove(&mut self, key: &str) -> Option<ParamValue> {
        self.parameters.remove(key)
    }

    /// Check whether a parameter exists.
    pub fn contains(&self, key: &str) -> bool {
        self.parameters.contains_key(key)
    }

    /// Number of stored parameters.
    pub fn len(&self) -> usize {
        self.parameters.len()
    }

    /// True when no parameters have been stored.
    pub fn is_empty(&self) -> bool {
        self.parameters.is_empty()
    }

    /// Get a float parameter by name, falling back to `default`.
    pub fn get_f32(&self, key: &str, default: f32) -> f32 {
        self.parameters
            .get(key)
            .and_then(|v| v.as_f32())
            .unwrap_or(default)
    }

    /// Get an integer parameter by name.
    pub fn get_i32(&self, key: &str, default: i32) -> i32 {
        self.parameters
            .get(key)
            .and_then(|v| v.as_i32())
            .unwrap_or(default)
    }

    /// Get a bool parameter by name.
    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.parameters
            .get(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(default)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_id_valid() {
        assert!(ParameterId::new("gain").is_ok());
        assert!(ParameterId::new("cutoff_freq").is_ok());
        assert!(ParameterId::new("delay_time_2").is_ok());
    }

    #[test]
    fn test_parameter_id_invalid() {
        assert!(ParameterId::new("").is_err());
        assert!(ParameterId::new("1gain").is_err());
        assert!(ParameterId::new("_gain").is_err());

        let long_name = "a".repeat(ParameterId::MAX_LEN + 1);
        assert!(ParameterId::new(long_name).is_err());
    }

    #[test]
    fn test_param_value_conversion() {
        let f = ParamValue::Float(42.0);
        assert_eq!(f.as_f32(), Some(42.0));
        assert_eq!(f.as_i32(), Some(42));
        assert_eq!(f.as_bool(), Some(true));

        let i = ParamValue::Int(0);
        assert_eq!(i.as_f32(), Some(0.0));
        assert_eq!(i.as_i32(), Some(0));
        assert_eq!(i.as_bool(), Some(false));
    }
}
