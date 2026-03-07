//! Port types and identifiers for the Kama Audio ecosystem
//!
//! Ports are the connection points between nodes in the audio graph.
//! Two types of signals flow through the graph:
//! - Audio signals (for sound)
//! - Control signals (for automation, LFOs, envelopes)
//! - Clock signals (for synchronization)

use crate::traits::node::NodeId;
use std::fmt;

// ============================================================================
// Port Type
// ============================================================================

/// Type of a port - what kind of signal it carries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortType {
    /// Audio signal port - carries sound (typically -1.0 to 1.0)
    Audio,
    
    /// Control signal port - carries modulation/automation
    Control,
    
    /// Clock signal port - carries timing information
    Clock,
    
    /// Feedback port - stores state between blocks
    Feedback,
    
    /// Parameter port - for node parameters (special)
    Param,
}

impl PortType {
    /// Get the name of the port type
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Control => "control",
            Self::Clock => "clock",
            Self::Feedback => "feedback",
            Self::Param => "param",
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
    
    /// Check if this port carries clock signals
    pub const fn is_clock(&self) -> bool {
        matches!(self, Self::Clock)
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
        Self::new(node, PortType::Audio, PortDirection::Input, index)
    }
    
    /// Create a new audio output port
    pub const fn audio_out(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Audio, PortDirection::Output, index)
    }
    
    // ========================================================================
    // Control Port Constructors
    // ========================================================================
    
    /// Create a new control input port
    pub const fn control_in(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Control, PortDirection::Input, index)
    }
    
    /// Create a new control output port
    pub const fn control_out(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Control, PortDirection::Output, index)
    }
    
    // ========================================================================
    // Clock Port Constructors
    // ========================================================================
    
    /// Create a new clock input port
    pub const fn clock_in(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Clock, PortDirection::Input, index)
    }
    
    /// Create a new clock output port
    pub const fn clock_out(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Clock, PortDirection::Output, index)
    }
    
    // ========================================================================
    // Feedback Port Constructors
    // ========================================================================
    
    /// Create a new feedback input port
    pub const fn feedback_in(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Feedback, PortDirection::Input, index)
    }
    
    /// Create a new feedback output port
    pub const fn feedback_out(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Feedback, PortDirection::Output, index)
    }
    
    // ========================================================================
    // Parameter Port Constructors
    // ========================================================================
    
    /// Create a new parameter port (always input)
    pub const fn param(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Param, PortDirection::Input, index)
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
    
    /// Check if this is a clock port
    pub const fn is_clock(&self) -> bool {
        matches!(self.port_type, PortType::Clock)
    }
    
    /// Check if this is a feedback port
    pub const fn is_feedback(&self) -> bool {
        matches!(self.port_type, PortType::Feedback)
    }
    
    /// Check if this is a parameter port
    pub const fn is_param(&self) -> bool {
        matches!(self.port_type, PortType::Param)
    }
}

impl fmt::Display for PortId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
    fn test_port_id_creation() {
        let node = NodeId(42);
        
        let audio_in = PortId::audio_in(node, 0);
        assert_eq!(audio_in.port_type(), PortType::Audio);
        assert!(audio_in.is_input());
        
        let clock_out = PortId::clock_out(node, 0);
        assert_eq!(clock_out.port_type(), PortType::Clock);
        assert!(clock_out.is_output());
        
        let feedback_in = PortId::feedback_in(node, 0);
        assert_eq!(feedback_in.port_type(), PortType::Feedback);
        assert!(feedback_in.is_input());
    }
}