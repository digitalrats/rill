// kama-core/src/traits/param.rs

use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// Ошибки валидации ParameterId.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ParameterError {
    #[error("Parameter name cannot be empty")]
    Empty,
    #[error("Parameter name cannot contain '{0}'")]
    InvalidCharacter(char),
    #[error("Parameter name cannot contain brackets")]
    InvalidBrackets,
    #[error("Parameter name too long (max {max} characters)")]
    TooLong { max: usize },
    #[error("Parameter name must start with a letter")]
    MustStartWithLetter,
}

/// Идентификатор параметра.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParameterId {
    name: String,
}

impl ParameterId {
    pub const MAX_LEN: usize = 64;

    pub fn new(name: impl Into<String>) -> Result<Self, ParameterError> {
        let name = name.into();
        // Валидация
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
            match c {
                'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => continue,
                '.' => return Err(ParameterError::InvalidCharacter('.')),
                '[' | ']' => return Err(ParameterError::InvalidBrackets),
                '/' => return Err(ParameterError::InvalidCharacter('/')),
                _ => return Err(ParameterError::InvalidCharacter(c)),
            }
        }
        Ok(Self { name })
    }

    #[cfg(test)]
    pub fn new_unchecked(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    pub fn as_str(&self) -> &str {
        &self.name
    }

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

#[cfg(feature = "serde")]
impl serde::Serialize for ParameterId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.name)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ParameterId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ParameterId::new(s).map_err(serde::de::Error::custom)
    }
}

/// Тип значения параметра.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    String(String),
    Choice(String),
}

/// Тип параметра.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ParamType {
    Float,
    Int,
    Bool,
    String,
    Choice,
}

/// Диапазон значений параметра.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ParamRange {
    pub min: Option<f32>,
    pub max: Option<f32>,
    pub step: Option<f32>,
}

impl ParamRange {
    pub fn new() -> Self {
        Self { min: None, max: None, step: None }
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

impl Default for ParamRange {
    fn default() -> Self {
        Self::new()
    }
}

/// Метаданные параметра.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ParamMetadata {
    pub name: String,
    pub typ: ParamType,
    pub default: ParamValue,
    pub range: ParamRange,
    pub unit: Option<String>,
    pub choices: Option<Vec<(String, f32)>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parameter_id_valid() {
        assert!(ParameterId::new("gain").is_ok());
        assert!(ParameterId::new("cutoff_freq").is_ok());
    }

    #[test]
    fn test_parameter_id_invalid() {
        assert!(ParameterId::new("").is_err());
        assert!(ParameterId::new("1gain").is_err());
        assert!(ParameterId::new("gain.value").is_err());
        assert!(ParameterId::new("gain[0]").is_err());
    }
}