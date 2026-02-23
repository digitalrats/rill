//! Типы и структуры для работы с параметрами узлов

use std::fmt;

/// Идентификатор параметра узла
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParameterId {
    /// Именованный параметр
    Name(String),
    /// Индексированный параметр (для массивов)
    Index(usize),
}

impl ParameterId {
    /// Создать новый идентификатор из имени
    pub fn from_name(name: impl Into<String>) -> Self {
        Self::Name(name.into())
    }
    
    /// Создать новый идентификатор из индекса
    pub fn from_index(index: usize) -> Self {
        Self::Index(index)
    }
    
    /// Получить строковое представление
    pub fn as_str(&self) -> &str {
        match self {
            Self::Name(s) => s.as_str(),
            Self::Index(_) => "", // для индексов нужно особое представление
        }
    }
}

impl From<String> for ParameterId {
    fn from(s: String) -> Self {
        Self::Name(s)
    }
}

impl From<&str> for ParameterId {
    fn from(s: &str) -> Self {
        Self::Name(s.to_string())
    }
}

impl From<usize> for ParameterId {
    fn from(i: usize) -> Self {
        Self::Index(i)
    }
}

impl fmt::Display for ParameterId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(name) => write!(f, "{}", name),
            Self::Index(idx) => write!(f, "[{}]", idx),
        }
    }
}

/// Тип значения параметра
#[derive(Debug, Clone, PartialEq)]
pub enum ParamValue {
    /// Числовое значение с плавающей точкой
    Float(f32),
    /// Целочисленное значение
    Int(i32),
    /// Логическое значение
    Bool(bool),
    /// Строковое значение
    String(String),
    /// Выбор из предопределённых вариантов
    Choice(String),
}

/// Тип параметра
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamType {
    /// Числовой с плавающей точкой
    Float,
    /// Целочисленный
    Int,
    /// Логический
    Bool,
    /// Строковый
    String,
    /// Выбор из вариантов
    Choice,
}

/// Диапазон значений параметра
#[derive(Debug, Clone, PartialEq)]
pub struct ParamRange {
    /// Минимальное значение
    pub min: Option<f32>,
    /// Максимальное значение
    pub max: Option<f32>,
    /// Шаг изменения
    pub step: Option<f32>,
}

impl ParamRange {
    /// Создать новый диапазон
    pub fn new() -> Self {
        Self {
            min: None,
            max: None,
            step: None,
        }
    }
    
    /// Установить минимальное значение
    pub fn with_min(mut self, min: f32) -> Self {
        self.min = Some(min);
        self
    }
    
    /// Установить максимальное значение
    pub fn with_max(mut self, max: f32) -> Self {
        self.max = Some(max);
        self
    }
    
    /// Установить шаг изменения
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

/// Метаданные параметра
#[derive(Debug, Clone, PartialEq)]
pub struct ParamMetadata {
    /// Имя параметра
    pub name: String,
    /// Тип параметра
    pub typ: ParamType,
    /// Значение по умолчанию
    pub default: ParamValue,
    /// Минимальное значение (для числовых типов)
    pub min: Option<f32>,
    /// Максимальное значение (для числовых типов)
    pub max: Option<f32>,
    /// Шаг изменения (для числовых типов)
    pub step: Option<f32>,
    /// Единица измерения
    pub unit: Option<String>,
    /// Возможные варианты выбора
    pub choices: Option<Vec<(String, f32)>>,
}