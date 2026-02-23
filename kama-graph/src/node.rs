use kama_core_traits::{param::ParamValue, AudioNode, NodeMetadata, NodeTypeId};
use std::any::Any;

/// Функция для создания узла
pub type NodeFactoryFn = fn() -> Box<dyn AudioNode>;

/// Информация о зарегистрированном типе узла
#[derive(Clone)]
pub struct NodeTypeInfo {
    /// Имя типа (для создания по строке)
    pub type_name: String,
    /// Метаданные узла
    pub metadata: NodeMetadata,
    /// ID типа
    pub type_id: NodeTypeId,
    /// Фабрика для создания узла
    pub factory: NodeFactoryFn,
}

impl NodeTypeInfo {
    /// Создать новый экземпляр узла
    pub fn create_node(&self) -> Box<dyn AudioNode> {
        (self.factory)()
    }
}

/// Сериализуемое представление параметра
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg(feature = "serde")]
pub struct SerializableParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub value: serde_json::Value,
}

/// Сериализуемое представление узла
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg(feature = "serde")]
pub struct SerializableNode {
    /// Имя типа узла
    pub type_name: String,
    /// ID узла (для ссылок)
    pub id: usize,
    /// Параметры узла
    pub parameters: Vec<SerializableParameter>,
}

/// Сериализуемое представление соединения
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg(feature = "serde")]
pub struct SerializableConnection {
    pub from_node: usize,
    pub from_port: u8,
    pub to_node: usize,
    pub to_port: u8,
    pub gain: f32,
}

/// Сериализуемое представление графа
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg(feature = "serde")]
pub struct SerializableGraph {
    pub nodes: Vec<SerializableNode>,
    pub connections: Vec<SerializableConnection>,
    pub sample_rate: f32,
}

/// Конвертация ParamValue в JSON
#[cfg(feature = "serde")]
impl SerializableParameter {
    pub fn from_param(name: &str, value: &ParamValue) -> Self {
        let (param_type, json_value) = match value {
            ParamValue::Float(f) => (
                "float",
                serde_json::Value::Number(
                    serde_json::Number::from_f64(*f as f64).unwrap_or(0.into()),
                ),
            ),
            ParamValue::Int(i) => (
                "int",
                serde_json::Value::Number(serde_json::Number::from(*i)),
            ),
            ParamValue::Bool(b) => ("bool", serde_json::Value::Bool(*b)),
            ParamValue::String(s) => ("string", serde_json::Value::String(s.clone())),
            ParamValue::Choice(s) => ("choice", serde_json::Value::String(s.clone())),
        };

        Self {
            name: name.to_string(),
            param_type: param_type.to_string(),
            value: json_value,
        }
    }

    pub fn to_param(&self) -> RegistryResult<ParamValue> {
        match self.param_type.as_str() {
            "float" => {
                let f = self
                    .value
                    .as_f64()
                    .ok_or_else(|| RegistryError::InvalidParameter("Invalid float value".into()))?;
                Ok(ParamValue::Float(f as f32))
            }
            "int" => {
                let i = self
                    .value
                    .as_i64()
                    .ok_or_else(|| RegistryError::InvalidParameter("Invalid int value".into()))?;
                Ok(ParamValue::Int(i as i32))
            }
            "bool" => {
                let b = self
                    .value
                    .as_bool()
                    .ok_or_else(|| RegistryError::InvalidParameter("Invalid bool value".into()))?;
                Ok(ParamValue::Bool(b))
            }
            "string" | "choice" => {
                let s = self.value.as_str().ok_or_else(|| {
                    RegistryError::InvalidParameter("Invalid string value".into())
                })?;
                if self.param_type == "choice" {
                    Ok(ParamValue::Choice(s.to_string()))
                } else {
                    Ok(ParamValue::String(s.to_string()))
                }
            }
            _ => Err(RegistryError::InvalidParameter(format!(
                "Unknown parameter type: {}",
                self.param_type
            ))),
        }
    }
}
