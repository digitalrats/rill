//! Идентификаторы портов для соединения узлов

use std::fmt;
use super::node::NodeId;

/// Идентификатор порта узла
///
/// Уникально идентифицирует конкретный порт (входной или выходной) узла в графе.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortId {
    /// ID узла, которому принадлежит порт
    pub node: NodeId,
    /// Индекс порта (0 для первого входа/выхода)
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
    
    /// Получить строковое представление для отладки
    pub fn as_str(&self) -> String {
        format!("{}", self)
    }
}

impl fmt::Display for PortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let port_type = if self.is_input { "in" } else { "out" };
        write!(f, "{}.{}[{}]", self.node, port_type, self.index)
    }
}

impl From<(NodeId, u8, bool)> for PortId {
    fn from((node, index, is_input): (NodeId, u8, bool)) -> Self {
        Self::new(node, index, is_input)
    }
}