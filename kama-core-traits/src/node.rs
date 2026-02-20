use std::fmt::Debug;
use std::hash::Hash;
use std::any::TypeId;
use std::fmt;

use crate::param::{ParamValue, ParamType, ParamMetadata};
use crate::error::AudioResult;

/// Идентификатор порта узла
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortId {
    /// ID узла
    pub node: NodeId,
    /// Индекс порта
    pub index: u8,
    /// true - входной порт, false - выходной
    pub is_input: bool,
}

impl PortId {
    /// Создать новый PortId
    pub fn new(node: NodeId, index: u8, is_input: bool) -> Self {
        Self { node, index, is_input }
    }
    
    /// Создать входной порт
    pub fn input(node: NodeId, index: u8) -> Self {
        Self { node, index, is_input: true }
    }
    
    /// Создать выходной порт
    pub fn output(node: NodeId, index: u8) -> Self {
        Self { node, index, is_input: false }
    }
    
    /// Проверить, является ли порт входным
    pub fn is_input(&self) -> bool {
        self.is_input
    }
    
    /// Проверить, является ли порт выходным
    pub fn is_output(&self) -> bool {
        !self.is_input
    }
}

impl fmt::Display for PortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let port_type = if self.is_input { "in" } else { "out" };
        write!(f, "{}.{}[{}]", self.node, port_type, self.index)
    }
}

/// Категории узлов
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// Метаданные узла
#[derive(Debug, Clone)]
pub struct NodeMetadata {
    pub name: String,
    pub category: NodeCategory,
    pub description: String,
    pub author: String,
    pub version: String,
    pub parameters: Vec<ParamMetadata>,
}

/// Уникальный идентификатор типа узла
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeTypeId(TypeId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.0)
    }
}

impl NodeTypeId {
    /// Создать идентификатор для типа T
    pub fn of<T: 'static>() -> Self {
        Self(TypeId::of::<T>())
    }
    
    /// Получить внутренний TypeId
    pub fn as_type_id(&self) -> TypeId {
        self.0
    }
}

/// Базовый трейт для всех аудиоузлов
pub trait AudioNode: Send + Sync {
    /// Уникальный идентификатор типа узла (для динамического вызова)
    fn node_type_id(&self) -> NodeTypeId;
    
    /// Получить идентификатор типа для данного типа T (для статических вызовов)
    /// 
    /// Этот метод доступен только для типов с 'static lifetime.
    fn static_type_id() -> NodeTypeId
    where
        Self: 'static + Sized,  // Добавляем 'static bound
    {
        NodeTypeId::of::<Self>()
    }
    
    /// Обработать аудио
    fn process(&mut self, inputs: &[&[f32]], outputs: &mut [&mut [f32]]) -> AudioResult<()>;
    
    /// Получить значение параметра
    fn get_param(&self, name: &str) -> Option<ParamValue>;
    
    /// Установить значение параметра
    fn set_param(&mut self, name: &str, value: ParamValue) -> AudioResult<()>;
    
    /// Инициализировать узел с частотой дискретизации
    fn init(&mut self, sample_rate: f32);
    
    /// Сбросить состояние узла
    fn reset(&mut self);
    
    /// Количество входов
    fn num_inputs(&self) -> usize;
    
    /// Количество выходов
    fn num_outputs(&self) -> usize;
    
    /// Получить метаданные узла
    fn metadata(&self) -> NodeMetadata;
}

/// Фабрика узлов
pub trait NodeCreator: Send + Sync {
    /// Создать экземпляр узла
    fn create(&self) -> Option<Box<dyn AudioNode>>;
    
    /// Получить метаданные узла
    fn metadata(&self) -> NodeMetadata;
    
    /// Получить идентификатор типа узла
    fn node_type_id(&self) -> NodeTypeId;
}