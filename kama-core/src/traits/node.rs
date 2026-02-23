// kama-core/src/traits/node.rs

use std::fmt;
use std::any::TypeId;
use super::error::{AudioError, AudioResult};
use super::param::{ParameterId, ParamValue, ParamMetadata};
use super::port::{PortId, PortType};

/// Идентификатор узла в графе.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeId(pub u32);

impl NodeId {
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    pub fn inner(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

impl From<u32> for NodeId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

/// Категория узла.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
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

/// Метаданные узла.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeMetadata {
    pub name: String,
    pub category: NodeCategory,
    pub description: String,
    pub author: String,
    pub version: String,
    pub parameters: Vec<ParamMetadata>,
}

/// Идентификатор типа узла (для type_id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeTypeId(TypeId);

impl NodeTypeId {
    pub fn of<T: 'static>() -> Self {
        Self(TypeId::of::<T>())
    }

    pub fn as_type_id(&self) -> TypeId {
        self.0
    }
}

/// Базовый трейт для всех аудиоузлов.
pub trait AudioNode: Send + Sync {
    /// Тип узла.
    fn node_type_id(&self) -> NodeTypeId;

    /// Обработка аудиоблока.
    fn process(
        &mut self,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) -> AudioResult<()>;

    /// Инициализация узла с частотой дискретизации.
    fn init(&mut self, sample_rate: f32);

    /// Сброс внутреннего состояния.
    fn reset(&mut self);

    /// Количество портов заданного типа.
    fn num_ports(&self, port_type: PortType) -> usize {
        if port_type == PortType::Node {
            1
        } else {
            0
        }
    }

    /// Получение значения параметра порта.
    fn get_port_param(&self, port: PortId, param: ParameterId) -> Option<ParamValue> {
        if port.port_type() == PortType::Node && port.index() == 0 {
            // Делегируем старому методу get_param, который ещё может существовать?
            // Но мы не хотим сохранять старый метод. Лучше оставить как заглушку.
            // В реальности узел должен переопределить этот метод для Node порта.
            None
        } else {
            None
        }
    }

    /// Установка значения параметра порта.
    fn set_port_param(
        &mut self,
        port: PortId,
        param: ParameterId,
        value: ParamValue,
    ) -> AudioResult<()> {
        if port.port_type() == PortType::Node && port.index() == 0 {
            // Аналогично.
            Err(AudioError::Parameter(format!("Node port not implemented")))
        } else {
            Err(AudioError::Parameter(format!("Port {} not supported", port)))
        }
    }

    /// Метаданные узла.
    fn metadata(&self) -> NodeMetadata;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id() {
        let id = NodeId(42);
        assert_eq!(id.inner(), 42);
        assert_eq!(format!("{}", id), "#42");
    }
}