//! # Базовые типы для графа обработки
//!
//! Этот модуль содержит базовые типы для построения аудиографа.
//! Полная реализация графа находится в крейте `kama-graph`.

use std::sync::Arc;

use crate::node::{AudioNode, NodeId, NodeState};
use crate::port::PortId;
use crate::buffer::PipeBuffer;
use crate::error::Result;
use crate::math::AudioNum;

/// Тип соединения между узлами
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionType {
    /// Точка-точка (один к одному)
    PointToPoint,
    /// Разветвитель (один ко многим)
    FanOut,
    /// Сумматор (многие к одному)
    FanIn,
}

/// Информация о соединении
#[derive(Debug, Clone)]
pub struct Connection {
    /// Выходной порт источника
    pub from: PortId,
    /// Входной порт назначения
    pub to: PortId,
    /// Тип соединения
    pub conn_type: ConnectionType,
    /// Коэффициент усиления (для сумматора/разветвителя)
    pub gain: f32,
}

impl Connection {
    /// Создать новое соединение
    pub fn new(from: PortId, to: PortId, gain: f32) -> Self {
        Self {
            from,
            to,
            conn_type: ConnectionType::PointToPoint,
            gain,
        }
    }
    
    /// Создать соединение с указанным типом
    pub fn with_type(mut self, conn_type: ConnectionType) -> Self {
        self.conn_type = conn_type;
        self
    }
}

/// Информация об узле в графе
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Идентификатор узла
    pub id: NodeId,
    /// Имя узла
    pub name: String,
    /// Тип узла
    pub node_type: crate::node::NodeCategory,
    /// Количество входных портов
    pub num_inputs: usize,
    /// Количество выходных портов
    pub num_outputs: usize,
    /// Количество управляющих портов
    pub num_controls: usize,
}

/// Базовый трейт для аудиографа
///
/// Полная реализация находится в крейте `kama-graph`.
/// Здесь определён только минимальный интерфейс для совместимости.
pub trait AudioGraph<T: AudioNum, const BUF_SIZE: usize>: Send + Sync {
    /// Добавить узел в граф
    fn add_node(&mut self, node: Box<dyn AudioNode<T, BUF_SIZE>>) -> NodeId;
    
    /// Удалить узел из графа
    fn remove_node(&mut self, id: NodeId) -> Option<Box<dyn AudioNode<T, BUF_SIZE>>>;
    
    /// Соединить два порта
    fn connect(&mut self, from: PortId, to: PortId, gain: f32) -> Result<()>;
    
    /// Разорвать соединение
    fn disconnect(&mut self, from: PortId, to: PortId) -> Result<()>;
    
    /// Получить информацию об узле
    fn node_info(&self, id: NodeId) -> Option<NodeInfo>;
    
    /// Получить список всех узлов
    fn nodes(&self) -> Vec<NodeInfo>;
    
    /// Получить список всех соединений
    fn connections(&self) -> Vec<Connection>;
    
    /// Обработать один блок
    fn process(&mut self) -> Result<()>;
    
    /// Получить состояние узла
    fn node_state(&self, id: NodeId) -> Option<NodeState<T,BUF_SIZE>>;
    
    /// Установить параметр узла
    fn set_parameter(&self, node_id: NodeId, name: &str, value: T) -> Result<()>;
}

/// Хендл для взаимодействия с графом из других потоков
#[derive(Clone)]
pub struct GraphHandle<T: AudioNum, const BUF_SIZE: usize> {
    inner: Arc<dyn AudioGraph<T, BUF_SIZE>>,
}

impl<T: AudioNum, const BUF_SIZE: usize> GraphHandle<T, BUF_SIZE> {
    /// Создать новый хендл
    pub fn new(graph: Arc<dyn AudioGraph<T, BUF_SIZE>>) -> Self {
        Self { inner: graph }
    }
    
    /// Установить параметр узла (RT-safe)
    pub fn set_parameter(&self, node_id: NodeId, name: &str, value: T) -> Result<()> {
        self.inner.set_parameter(node_id, name, value)
    }
    
    /// Получить информацию об узле
    pub fn node_info(&self, id: NodeId) -> Option<NodeInfo> {
        self.inner.node_info(id)
    }
    
    /// Получить состояние узла
    pub fn node_state(&self, id: NodeId) -> Option<NodeState<T,BUF_SIZE>> {
        self.inner.node_state(id)
    }
}