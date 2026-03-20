//! Port types and identifiers for the Kama Audio ecosystem
//!
//! Ports are the connection points between nodes in the audio graph.
//! Two types of signals flow through the graph:
//! - Audio signals (for sound)
//! - Control signals (for automation, LFOs, envelopes)
//! - Clock signals (for synchronization)

use crate::traits::node::NodeId;
use crate::buffer::PipeBuffer;
use crate::error::{Error, ErrorCode};
use crate::traits::error::PortError;
use crate::time::ClockTick;
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
// Port Structure (for runtime)
// ============================================================================

/// A port on a node
#[derive(Debug, Clone)]
pub struct Port<T: crate::math::AudioNum, const BUF_SIZE: usize> {
    /// Port identifier
    pub id: PortId,
    /// Port name
    pub name: String,
    /// Connection buffer (if connected)
    pub buffer: Option<PipeBuffer<T, BUF_SIZE>>,
}

impl<T: crate::math::AudioNum, const BUF_SIZE: usize> Port<T, BUF_SIZE> {
    /// Create a new input port
    pub fn input(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::audio_in(node_id, index),
            name: name.to_string(),
            buffer: None,
        }
    }
    
    /// Create a new output port
    pub fn output(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::audio_out(node_id, index),
            name: name.to_string(),
            buffer: None,
        }
    }
    
    /// Create a new control input port
    pub fn control_in(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::control_in(node_id, index),
            name: name.to_string(),
            buffer: None,
        }
    }
    
    /// Create a new control output port
    pub fn control_out(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::control_out(node_id, index),
            name: name.to_string(),
            buffer: None,
        }
    }
    
    /// Get the port ID
    pub fn id(&self) -> PortId {
        self.id
    }
    
    /// Get the port name
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Check if port is connected
    pub fn is_connected(&self) -> bool {
        self.buffer.is_some()
    }
    
    /// Connect to a buffer
    pub fn connect(&mut self, buffer: PipeBuffer<T, BUF_SIZE>) {
        self.buffer = Some(buffer);
    }
    
    /// Disconnect from buffer
    pub fn disconnect(&mut self) {
        self.buffer = None;
    }
    
    /// Read data from port into provided buffer (for input ports)
    pub fn read(&self, output: &mut [T; BUF_SIZE]) -> Result<(), Error> {
        match &self.buffer {
            Some(buffer) => {
                if let Some(data) = buffer.try_read() {
                    *output = data;
                    Ok(())
                } else {
                    Err(Error::new(
                        ErrorCode::BufferEmpty,
                        "No data available in port",
                    ))
                }
            }
            None => Err(Error::new(
                ErrorCode::BufferEmpty,
                "Port not connected",
            )),
        }
    }
    
    /// Write data to port (for output ports)
    pub fn write(&mut self, data: &[T; BUF_SIZE]) -> Result<(), Error> {
        match &mut self.buffer {
            Some(buffer) => {
                buffer.write(data);
                Ok(())
            }
            None => Err(Error::new(
                ErrorCode::BufferEmpty,
                "Port not connected",
            )),
        }
    }
}

// ============================================================================
// Active Port Trait
// ============================================================================

/// Trait for ports that can actively pull/push data.
pub trait ActivePort<T: crate::math::AudioNum, const BUF_SIZE: usize> {
    /// Pull data from the port (for input ports).
    ///
    /// Returns `Some` if data is available, `None` if port is disconnected
    /// or buffer empty.
    fn pull(&mut self) -> Option<[T; BUF_SIZE]>;

    /// Push data into the port (for output ports).
    ///
    /// Returns `Ok(())` on success, `Err(PortError)` if port is disconnected
    /// or buffer full.
    fn push(&mut self, data: [T; BUF_SIZE]) -> Result<(), PortError>;

    /// Check if the port is connected (has a buffer).
    fn is_connected(&self) -> bool;

    /// Called on each clock tick (optional).
    fn on_tick(&mut self, _tick: &ClockTick) {
        // Default implementation does nothing
    }
}

impl<T: crate::math::AudioNum, const BUF_SIZE: usize> ActivePort<T, BUF_SIZE> for Port<T, BUF_SIZE> {
    #[inline]
    fn pull(&mut self) -> Option<[T; BUF_SIZE]> {
        self.buffer.as_ref()?.try_read()
    }

    #[inline]
    fn push(&mut self, data: [T; BUF_SIZE]) -> Result<(), PortError> {
        match &mut self.buffer {
            Some(buffer) => {
                buffer.write(&data);
                Ok(())
            }
            None => Err(PortError::NotFound(self.id.to_string())),
        }
    }

    #[inline]
    fn is_connected(&self) -> bool {
        self.buffer.is_some()
    }

    #[inline]
    fn on_tick(&mut self, _tick: &ClockTick) {
        // Default does nothing
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