//! Port types and identifiers

use crate::traits::node::NodeId;
use std::fmt;

// ============================================================================
// Port Type
// ============================================================================

/// Type of a port - only audio and control signals flow through the graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortType {
    /// Audio signal port - carries sound (typically -1.0 to 1.0)
    Audio,
    
    /// Control signal port - carries modulation/automation (typically 0.0 to 1.0 or -1.0 to 1.0)
    Control,
}

impl PortType {
    /// Get the name of the port type
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Control => "control",
        }
    }
    
    /// Check if this port carries audio-rate signals
    pub const fn is_audio_rate(&self) -> bool {
        matches!(self, Self::Audio)
    }
    
    /// Check if this port carries control-rate signals
    pub const fn is_control_rate(&self) -> bool {
        matches!(self, Self::Control)
    }
}

impl fmt::Display for PortType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// Port Direction
// ============================================================================

/// Direction of a port (input or output)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortDirection {
    /// Input port (receives data into the node)
    Input,
    
    /// Output port (sends data out of the node)
    Output,
}

impl PortDirection {
    /// Get the name of the direction
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }
    
    /// Check if this is an input port
    pub const fn is_input(&self) -> bool {
        matches!(self, Self::Input)
    }
    
    /// Check if this is an output port
    pub const fn is_output(&self) -> bool {
        matches!(self, Self::Output)
    }
}

impl fmt::Display for PortDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// Port ID
// ============================================================================

/// Unique identifier for a port within a graph
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortId {
    node: NodeId,
    port_type: PortType,
    direction: PortDirection,
    index: u16,
}

impl PortId {
    /// Create a new port ID
    pub const fn new(
        node: NodeId,
        port_type: PortType,
        direction: PortDirection,
        index: u16,
    ) -> Self {
        Self {
            node,
            port_type,
            direction,
            index,
        }
    }
    
    // ========================================================================
    // Audio Port Constructors
    // ========================================================================
    
    /// Create a new audio input port
    pub const fn audio_in(node: NodeId, index: u16) -> Self {
        Self {
            node,
            port_type: PortType::Audio,
            direction: PortDirection::Input,
            index,
        }
    }
    
    /// Create a new audio output port
    pub const fn audio_out(node: NodeId, index: u16) -> Self {
        Self {
            node,
            port_type: PortType::Audio,
            direction: PortDirection::Output,
            index,
        }
    }
    
    // ========================================================================
    // Control Port Constructors
    // ========================================================================
    
    /// Create a new control input port
    pub const fn control_in(node: NodeId, index: u16) -> Self {
        Self {
            node,
            port_type: PortType::Control,
            direction: PortDirection::Input,
            index,
        }
    }
    
    /// Create a new control output port
    pub const fn control_out(node: NodeId, index: u16) -> Self {
        Self {
            node,
            port_type: PortType::Control,
            direction: PortDirection::Output,
            index,
        }
    }
    
    // ========================================================================
    // Convenience constructors
    // ========================================================================
    
    /// Create a new audio input port (alias)
    pub const fn input(node: NodeId, index: u16) -> Self {
        Self::audio_in(node, index)
    }
    
    /// Create a new audio output port (alias)
    pub const fn output(node: NodeId, index: u16) -> Self {
        Self::audio_out(node, index)
    }
    
    // ========================================================================
    // Getters
    // ========================================================================
    
    /// Get the node ID
    pub const fn node_id(&self) -> NodeId {
        self.node
    }
    
    /// Get the port type
    pub const fn port_type(&self) -> PortType {
        self.port_type
    }
    
    /// Get the port direction
    pub const fn direction(&self) -> PortDirection {
        self.direction
    }
    
    /// Get the port index
    pub const fn index(&self) -> u16 {
        self.index
    }
    
    // ========================================================================
    // Predicates
    // ========================================================================
    
    /// Check if this is an input port
    pub const fn is_input(&self) -> bool {
        self.direction.is_input()
    }
    
    /// Check if this is an output port
    pub const fn is_output(&self) -> bool {
        self.direction.is_output()
    }
    
    /// Check if this is an audio port
    pub const fn is_audio(&self) -> bool {
        matches!(self.port_type, PortType::Audio)
    }
    
    /// Check if this is a control port
    pub const fn is_control(&self) -> bool {
        matches!(self.port_type, PortType::Control)
    }
}

impl fmt::Display for PortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Format as: Node(42).audio_in[0]
        write!(
            f,
            "Node({}).{}_{}[{}]",
            self.node.inner(),
            self.port_type.name(),
            self.direction.name(),
            self.index
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_port_type() {
        assert_eq!(PortType::Audio.name(), "audio");
        assert_eq!(PortType::Control.name(), "control");
        assert!(PortType::Audio.is_audio_rate());
        assert!(PortType::Control.is_control_rate());
        assert!(!PortType::Audio.is_control_rate());
    }
    
    #[test]
    fn test_port_direction() {
        assert!(PortDirection::Input.is_input());
        assert!(!PortDirection::Input.is_output());
        assert!(PortDirection::Output.is_output());
        assert!(!PortDirection::Output.is_input());
    }
    
    #[test]
    fn test_port_id_creation() {
        let node = NodeId(42);
        
        let audio_in = PortId::audio_in(node, 0);
        assert_eq!(audio_in.node_id(), node);
        assert_eq!(audio_in.port_type(), PortType::Audio);
        assert_eq!(audio_in.direction(), PortDirection::Input);
        assert_eq!(audio_in.index(), 0);
        assert!(audio_in.is_input());
        assert!(!audio_in.is_output());
        assert!(audio_in.is_audio());
        assert!(!audio_in.is_control());
        
        let audio_out = PortId::audio_out(node, 1);
        assert_eq!(audio_out.node_id(), node);
        assert_eq!(audio_out.port_type(), PortType::Audio);
        assert_eq!(audio_out.direction(), PortDirection::Output);
        assert_eq!(audio_out.index(), 1);
        assert!(!audio_out.is_input());
        assert!(audio_out.is_output());
        
        let control_in = PortId::control_in(node, 0);
        assert_eq!(control_in.port_type(), PortType::Control);
        assert!(control_in.is_control());
        assert!(!control_in.is_audio());
        
        let control_out = PortId::control_out(node, 1);
        assert_eq!(control_out.port_type(), PortType::Control);
        assert!(control_out.is_output());
    }
    
    #[test]
    fn test_port_id_display() {
        let node = NodeId(42);
        let port = PortId::audio_in(node, 0);
        assert_eq!(format!("{}", port), "Node(42).audio_input[0]");
        
        let port = PortId::control_out(node, 1);
        assert_eq!(format!("{}", port), "Node(42).control_output[1]");
    }
}