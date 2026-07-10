//! Port types and identifiers for the Rill ecosystem
//!
//! Ports are the connection points between nodes in the signal graph.
//! Each output port owns a `FixedBuffer<T, BUF_SIZE>`. Input ports are
//! connection endpoints that receive data from upstream output ports.

use crate::buffer::FixedBuffer;
use crate::math::Transcendental;
use crate::traits::node::NodeId;
use std::fmt;

// ============================================================================
// Port Type
// ============================================================================

/// Type of a port - what kind of signal it carries
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortType {
    /// Signal port - carries signal blocks (signal data, sensor data, etc.)
    Signal,

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
            Self::Signal => "signal",
            Self::Control => "control",
            Self::Clock => "clock",
            Self::Feedback => "feedback",
            Self::Param => "param",
        }
    }

    /// Check if this port carries signal-rate signals
    pub const fn is_signal_rate(&self) -> bool {
        matches!(self, Self::Signal)
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
    // Signal Port Constructors
    // ========================================================================

    /// Create a new signal input port
    pub const fn signal_in(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Signal, PortDirection::Input, index)
    }

    /// Create a new signal output port
    pub const fn signal_out(node: NodeId, index: u16) -> Self {
        Self::new(node, PortType::Signal, PortDirection::Output, index)
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

    /// Check if this is a signal port
    pub const fn is_signal(&self) -> bool {
        matches!(self.port_type, PortType::Signal)
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
// Port Structure
// ============================================================================

/// A port on a node.
///
/// Each port has an owned `FixedBuffer<T, BUF_SIZE>` for its data.
/// Ports can optionally participate in feedback edges via `feedback_buffer`.
pub struct Port<T: Transcendental, const BUF_SIZE: usize> {
    /// Port identifier
    pub id: PortId,
    /// Port name
    pub name: String,
    /// Port direction (input/output)
    pub direction: PortDirection,
    /// Pending command value from the control path
    pending_command: Option<T>,
    /// Owned signal buffer (for output ports and input ports without upstream).
    buffer: FixedBuffer<T, BUF_SIZE>,
    /// Delayed feedback state (None if not on a feedback edge)
    feedback_buffer: Option<FixedBuffer<T, BUF_SIZE>>,
    /// Whether this input port has received new data in the current graph cycle.
    data_received: bool,
}

impl<T: Transcendental, const BUF_SIZE: usize> fmt::Debug for Port<T, BUF_SIZE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Port")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("direction", &self.direction)
            .field("has_feedback", &self.feedback_buffer.is_some())
            .finish()
    }
}

impl<T: Transcendental, const BUF_SIZE: usize> Port<T, BUF_SIZE> {
    /// Create a new signal output port
    pub fn output(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::signal_out(node_id, index),
            name: name.to_string(),
            direction: PortDirection::Output,
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            data_received: false,
        }
    }

    /// Create a new signal input port
    pub fn input(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::signal_in(node_id, index),
            name: name.to_string(),
            direction: PortDirection::Input,
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            data_received: false,
        }
    }

    /// Create a new control output port
    pub fn control_output(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::control_out(node_id, index),
            name: name.to_string(),
            direction: PortDirection::Output,
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            data_received: false,
        }
    }

    /// Create a new control output port with an algorithm
    pub fn control_output_with_action(
        node_id: NodeId,
        index: u16,
        name: &str,
        _action: Box<dyn crate::traits::algorithm::Algorithm<T>>,
    ) -> Self {
        Self {
            id: PortId::control_out(node_id, index),
            name: name.to_string(),
            direction: PortDirection::Output,
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            data_received: false,
        }
    }

    /// Create a new control input port
    pub fn control_input(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::control_in(node_id, index),
            name: name.to_string(),
            direction: PortDirection::Input,
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            data_received: false,
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

    /// Check if port is an input
    pub fn is_input(&self) -> bool {
        self.direction.is_input()
    }

    /// Check if port is an output
    pub fn is_output(&self) -> bool {
        self.direction.is_output()
    }

    /// Low-level access to the port's own buffer (engine / I-O boundary).
    pub fn buffer(&self) -> &FixedBuffer<T, BUF_SIZE> {
        &self.buffer
    }

    /// Low-level mutable access to the port's own buffer (crate-internal).
    pub(crate) fn buffer_mut(&mut self) -> &mut FixedBuffer<T, BUF_SIZE> {
        &mut self.buffer
    }

    /// Read the effective input block for this port.
    #[inline]
    pub fn read(&self) -> &[T; BUF_SIZE] {
        self.buffer.as_array()
    }

    /// Mutable access to this port's output block.
    #[inline]
    pub fn write(&mut self) -> &mut [T; BUF_SIZE] {
        self.buffer_mut().as_mut_array()
    }

    /// Write this port's output block from a source array.
    #[inline]
    pub fn write_from(&mut self, src: &[T; BUF_SIZE]) {
        self.write().copy_from_slice(src);
    }

    /// The delayed feedback block, if this port is on a feedback edge.
    #[inline]
    pub fn feedback(&self) -> Option<&[T; BUF_SIZE]> {
        self.feedback_buffer.as_ref().map(|b| b.as_array())
    }

    /// Get the signal buffer for this port.
    #[inline]
    pub fn signal_buffer(&self) -> &FixedBuffer<T, BUF_SIZE> {
        &self.buffer
    }

    /// Set a command value for this port.
    ///
    /// The value is stored as a pending command for delivery to the algorithm
    /// on the next processing cycle.
    pub fn set_value(&mut self, value: T) {
        self.pending_command = Some(value);
    }

    /// Consume any pending command value, returning it.
    pub fn take_pending_command(&mut self) -> Option<T> {
        self.pending_command.take()
    }

    // ========================================================================
    // Sink-facing status
    // ========================================================================

    /// Whether this input port received fresh data in the current graph cycle.
    #[inline]
    pub fn data_received(&self) -> bool {
        self.data_received
    }

    /// Set the `data_received` flag (sinks reset it after consuming).
    #[inline]
    pub fn set_data_received(&mut self, value: bool) {
        self.data_received = value;
    }

    /// Allocate this port's feedback buffer (feedback edge endpoint).
    #[inline]
    pub fn init_feedback_buffer(&mut self) {
        self.feedback_buffer = Some(FixedBuffer::new());
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

        let signal_in = PortId::signal_in(node, 0);
        assert_eq!(signal_in.port_type(), PortType::Signal);
        assert!(signal_in.is_input());

        let clock_out = PortId::clock_out(node, 0);
        assert_eq!(clock_out.port_type(), PortType::Clock);
        assert!(clock_out.is_output());

        let feedback_in = PortId::feedback_in(node, 0);
        assert_eq!(feedback_in.port_type(), PortType::Feedback);
        assert!(feedback_in.is_input());
    }
}
