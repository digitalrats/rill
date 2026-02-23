// kama-core/src/traits/port.rs

use std::fmt;
use super::node::NodeId;

/// Тип порта.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PortType {
    /// Порт самого узла (для параметров узла)
    Node,
    /// Входной аудиопорт
    AudioIn,
    /// Выходной аудиопорт
    AudioOut,
    /// Управляющий порт (для автоматизации)
    Control,
    /// CV вход
    CvIn,
    /// CV выход
    CvOut,
    /// Тактовый порт
    Clock,
    /// Триггерный порт
    Trigger,
}

impl PortType {
    pub fn name(&self) -> &'static str {
        match self {
            PortType::Node => "node",
            PortType::AudioIn => "audio_in",
            PortType::AudioOut => "audio_out",
            PortType::Control => "control",
            PortType::CvIn => "cv_in",
            PortType::CvOut => "cv_out",
            PortType::Clock => "clock",
            PortType::Trigger => "trigger",
        }
    }

    pub fn is_input(&self) -> bool {
        matches!(self,
            PortType::AudioIn | PortType::Control | PortType::CvIn |
            PortType::Clock | PortType::Trigger
        )
    }

    pub fn is_output(&self) -> bool {
        matches!(self, PortType::AudioOut | PortType::CvOut)
    }

    /// Для CV портов возвращает направление (true - вход, false - выход)
    pub fn cv_direction(&self) -> Option<bool> {
        match self {
            PortType::CvIn => Some(true),
            PortType::CvOut => Some(false),
            _ => None,
        }
    }
}

/// Идентификатор порта.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PortId {
    /// Узел, которому принадлежит порт.
    pub node: NodeId,
    /// Индекс порта (уникален в рамках узла для данного типа).
    pub index: u16,
    /// Тип порта.
    pub port_type: PortType,
}

impl PortId {
    pub fn new(node: NodeId, index: u16, port_type: PortType) -> Self {
        Self { node, index, port_type }
    }

    pub fn node(node: NodeId) -> Self {
        Self::new(node, 0, PortType::Node)
    }

    pub fn audio_in(node: NodeId, index: u16) -> Self {
        Self::new(node, index, PortType::AudioIn)
    }

    pub fn audio_out(node: NodeId, index: u16) -> Self {
        Self::new(node, index, PortType::AudioOut)
    }

    pub fn control(node: NodeId, index: u16) -> Self {
        Self::new(node, index, PortType::Control)
    }

    pub fn cv_in(node: NodeId, index: u16) -> Self {
        Self::new(node, index, PortType::CvIn)
    }

    pub fn cv_out(node: NodeId, index: u16) -> Self {
        Self::new(node, index, PortType::CvOut)
    }

    pub fn clock(node: NodeId, index: u16) -> Self {
        Self::new(node, index, PortType::Clock)
    }

    pub fn trigger(node: NodeId, index: u16) -> Self {
        Self::new(node, index, PortType::Trigger)
    }

    pub fn node_id(&self) -> NodeId {
        self.node
    }

    pub fn index(&self) -> u16 {
        self.index
    }

    pub fn port_type(&self) -> PortType {
        self.port_type
    }

    pub fn is_input(&self) -> bool {
        self.port_type.is_input()
    }

    pub fn is_output(&self) -> bool {
        self.port_type.is_output()
    }
}

impl fmt::Display for PortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}[{}]", self.node, self.port_type.name(), self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_type() {
        assert!(PortType::AudioIn.is_input());
        assert!(!PortType::AudioIn.is_output());
        assert_eq!(PortType::AudioIn.name(), "audio_in");
        assert_eq!(PortType::Control.name(), "control");
    }

    #[test]
    fn test_port_id() {
        let node = NodeId(42);
        let port = PortId::audio_in(node, 0);
        assert_eq!(port.node_id(), node);
        assert_eq!(port.index(), 0);
        assert_eq!(port.port_type(), PortType::AudioIn);
        assert!(port.is_input());
        assert!(!port.is_output());
        assert_eq!(port.to_string(), "#42.audio_in[0]");
    }
}