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
    #[error("Parameter name too long (max {max} characters)")]
    TooLong { max: usize },
    #[error("Parameter name must start with a letter")]
    MustStartWithLetter,
}

/// Идентификатор параметра с валидацией.
///
/// # Примеры
/// ```
/// # use kama_core::traits::ParameterId;
/// let gain = ParameterId::new("gain").unwrap();
/// let cutoff = ParameterId::new("cutoff_freq").unwrap();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ParameterId {
    name: String,
}

impl ParameterId {
    /// Максимальная длина имени параметра.
    pub const MAX_LEN: usize = 64;
    
    /// Создает новый ParameterId с валидацией.
    ///
    /// # Правила валидации
    /// - Не пустой
    /// - Не длиннее MAX_LEN
    /// - Начинается с буквы (a-z, A-Z)
    /// - Содержит только буквы, цифры и подчеркивания
    pub fn new(name: impl Into<String>) -> Result<Self, ParameterError> {
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
    
    /// Возвращает строковое представление.
    pub fn as_str(&self) -> &str {
        &self.name
    }
    
    /// Преобразует в строку.
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

/// Тип значения параметра.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    String(String),
    Choice(String), // Выбор из предопределенного списка
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
    /// Имя параметра (должно быть валидным ParameterId)
    pub name: String,
    /// Тип параметра
    pub typ: ParamType,
    /// Значение по умолчанию
    pub default: ParamValue,
    /// Диапазон значений
    pub range: ParamRange,
    /// Единица измерения (опционально)
    pub unit: Option<String>,
    /// Возможные варианты выбора (для Choice)
    pub choices: Option<Vec<(String, f32)>>,
}

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
        assert!(ParameterId::new("gain.value").is_err());
        assert!(ParameterId::new("cutoff-freq").is_err());
        
        let long_name = "a".repeat(ParameterId::MAX_LEN + 1);
        assert!(ParameterId::new(long_name).is_err());
    }
}