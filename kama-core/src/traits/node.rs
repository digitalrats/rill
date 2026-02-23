//! Типы и трейты для узлов обработки

use std::fmt;
use std::any::TypeId;

use super::error::AudioResult;
use super::param::{ParamValue, ParamMetadata};

/// Категории узлов
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeCategory {
    /// Генераторы сигнала (осцилляторы, шум)
    Generator,
    /// Эффекты (дилей, дисторшн)
    Effect,
    /// Фильтры
    Filter,
    /// Микшеры
    Mixer,
    /// Вспомогательные узлы
    Utility,
    /// Анализаторы
    Analyzer,
    /// MIDI-узлы
    Midi,
    /// Секвенсоры
    Sequencer,
}

/// Метаданные узла
#[derive(Debug, Clone)]
pub struct NodeMetadata {
    /// Имя узла
    pub name: String,
    /// Категория узла
    pub category: NodeCategory,
    /// Описание
    pub description: String,
    /// Автор
    pub author: String,
    /// Версия
    pub version: String,
    /// Параметры узла
    pub parameters: Vec<ParamMetadata>,
}

/// Уникальный идентификатор узла в графе
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u32);

impl NodeId {
    /// Создать новый идентификатор
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({})", self.0)
    }
}

/// Идентификатор типа узла
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeTypeId(TypeId);

impl NodeTypeId {
    /// Создать идентификатор для типа T
    pub fn of<T: 'static>() -> Self {
        Self(TypeId::of::<T>())
    }
}

/// Базовый трейт для всех аудиоузлов
pub trait AudioNode: Send + Sync {
    /// Уникальный идентификатор типа узла
    fn node_type_id(&self) -> NodeTypeId;
    
    /// Обработать аудиоблок
    fn process(
        &mut self,
        inputs: &[&[f32]],
        outputs: &mut [&mut [f32]],
    ) -> AudioResult<()>;
    
    /// Получить значение параметра
    fn get_param(&self, name: &str) -> Option<ParamValue>;
    
    /// Установить значение параметра
    fn set_param(&mut self, name: &str, value: ParamValue) -> AudioResult<()>;
    
    /// Инициализировать узел с заданной частотой дискретизации
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