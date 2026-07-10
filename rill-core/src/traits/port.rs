#![allow(deprecated)]
//! Port types and identifiers for the Rill ecosystem
//!
//! Ports are the connection points between nodes in the signal graph.
//! Each output port owns a `FixedBuffer<T, BUF_SIZE>` and an optional `Action`
//! that defines how data is produced. Input ports are connection endpoints
//! that receive data from upstream output ports.

use crate::buffer::{Buffer, FixedBuffer};
use crate::math::vector::scalar::ScalarVector4;
use crate::math::vector::traits::Vector as VecTrait;
use crate::math::Transcendental;
use crate::time::RenderContext;
use crate::traits::algorithm::Algorithm;
use crate::traits::node::NodeId;
use crate::traits::processable::Processable;
use crate::traits::{Node, ProcessResult};
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
/// Each port has an owned `FixedBuffer<T, BUF_SIZE>` for its data and an optional
/// `Action` that defines per-port processing. Output ports typically have
/// an action; input ports may have one for preprocessing.
///
/// Ports can optionally participate in feedback edges:
/// - On an output port in a feedback edge, `feedback_buffer` stores the
///   previous block's output, snapshotted after DSP via `snapshot_feedback()`.
/// - On an input port in a feedback edge, `feedback_buffer` holds the delayed
///   feedback value that gets mixed into `buffer` by `pre_process()`.
/// - `downstream` lists signal connections from this output port to input ports
///   of other nodes, populated at build time by the graph builder.
/// - `upstream_buffer` on input ports: direct pointer to the upstream output
///   port's buffer for zero-copy routing on **exclusive 1:1** edges only.
///   `None` for fan-in, fan-out, and feedback ports (each materializes an
///   independent copy).
///
/// # Safety
/// `upstream_buffer` is safe because the graph topology is immutable and
/// processing is strictly single-threaded in topological order. The
/// upstream output buffer is guaranteed to outlive the downstream input
/// port that references it.
pub struct Port<T: Transcendental, const BUF_SIZE: usize> {
    /// Port identifier
    pub id: PortId,
    /// Port name
    pub name: String,
    /// Port direction (input/output)
    pub direction: PortDirection,
    /// Per-port processing algorithm (None for simple input ports)
    action: Option<Box<dyn Algorithm<T>>>,
    /// Pending command value from the control path
    pending_command: Option<T>,
    /// Owned signal buffer (for output ports and input ports without upstream).
    ///
    /// Private: nodes access signal data through [`read`](Self::read) /
    /// [`write`](Self::write) / [`write_from`](Self::write_from) /
    /// [`feedback`](Self::feedback), which encapsulate the zero-copy routing.
    /// The engine uses [`buffer`](Self::buffer) / `buffer_mut`.
    buffer: FixedBuffer<T, BUF_SIZE>,
    /// Delayed feedback state (None if not on a feedback edge)
    feedback_buffer: Option<FixedBuffer<T, BUF_SIZE>>,
    /// Downstream signal connections: (target_node_index, target_port_index).
    /// Used for serialization and by `GraphBuilder::build()`.
    downstream: Vec<(usize, usize)>,
    /// Direct pointers to downstream input ports. Filled by
    /// `GraphBuilder::build()`. Used by `propagate` to copy data.
    downstream_input_ptrs: Vec<*mut Port<T, BUF_SIZE>>,
    /// Unique downstream nodes (one per target, deduplicated at build time).
    /// Filled by `GraphBuilder::build()`. Used by `propagate` to recurse
    /// into downstream nodes — no runtime deduplication needed.
    downstream_nodes: Vec<*mut crate::traits::NodeVariant<T, BUF_SIZE>>,
    /// Direct pointer to upstream output buffer for zero-copy routing.
    /// `Some` only for an **exclusive 1:1** input port (its upstream output
    /// has a single consumer and this input has a single producer).
    /// `None` for output ports, fan-in, fan-out branches, or unconnected —
    /// those materialize an independent copy.
    /// Valid for the engine's lifetime.
    upstream_buffer: Option<*const FixedBuffer<T, BUF_SIZE>>,
    /// Feedback edge targets from this output port (for serialization)
    feedback_downstream: Vec<(usize, usize)>,

    /// Direct pointers to `feedback_buffer` on downstream input ports.
    ///
    /// Set by `GraphBuilder::build()` for feedback edges.
    /// `snapshot_feedback()` copies its buffer into each target.
    feedback_ptrs: Vec<*mut Option<FixedBuffer<T, BUF_SIZE>>>,

    /// Whether this input port has received new data in the current graph cycle.
    ///
    /// Set by `propagate` when a downstream input port receives a buffer copy.
    /// Consumer nodes (esp. Sinks) check this flag to decide whether all
    /// input channels are fresh before producing output.
    data_received: bool,

    /// Pull-model: pointer to the upstream node that feeds this input port.
    /// Only set for same-chain edges (recording→recording or playback→playback).
    /// Used by the pull traversal in `process_playback_chain`.
    upstream_node: *mut crate::traits::NodeVariant<T, BUF_SIZE>,
}

impl<T: Transcendental, const BUF_SIZE: usize> fmt::Debug for Port<T, BUF_SIZE> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Port")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("direction", &self.direction)
            .field("has_action", &self.action.is_some())
            .field("has_feedback", &self.feedback_buffer.is_some())
            .field("downstream_len", &self.downstream.len())
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
            action: None,
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            downstream: Vec::new(),
            feedback_downstream: Vec::new(),
            feedback_ptrs: Vec::new(),
            downstream_input_ptrs: Vec::new(),
            downstream_nodes: Vec::new(),
            upstream_buffer: None,
            upstream_node: std::ptr::null_mut(),
            data_received: false,
        }
    }

    /// Create a new signal input port
    pub fn input(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::signal_in(node_id, index),
            name: name.to_string(),
            direction: PortDirection::Input,
            action: None,
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            downstream: Vec::new(),
            feedback_downstream: Vec::new(),
            feedback_ptrs: Vec::new(),
            downstream_input_ptrs: Vec::new(),
            downstream_nodes: Vec::new(),
            upstream_buffer: None,
            upstream_node: std::ptr::null_mut(),
            data_received: false,
        }
    }

    /// Create a new control output port
    pub fn control_output(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::control_out(node_id, index),
            name: name.to_string(),
            direction: PortDirection::Output,
            action: None,
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            downstream: Vec::new(),
            feedback_downstream: Vec::new(),
            feedback_ptrs: Vec::new(),
            downstream_input_ptrs: Vec::new(),
            downstream_nodes: Vec::new(),
            upstream_buffer: None,
            upstream_node: std::ptr::null_mut(),
            data_received: false,
        }
    }

    /// Create a new control output port with an algorithm
    pub fn control_output_with_action(
        node_id: NodeId,
        index: u16,
        name: &str,
        action: Box<dyn Algorithm<T>>,
    ) -> Self {
        Self {
            id: PortId::control_out(node_id, index),
            name: name.to_string(),
            direction: PortDirection::Output,
            action: Some(action),
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            downstream: Vec::new(),
            feedback_downstream: Vec::new(),
            feedback_ptrs: Vec::new(),
            downstream_input_ptrs: Vec::new(),
            downstream_nodes: Vec::new(),
            upstream_buffer: None,
            upstream_node: std::ptr::null_mut(),
            data_received: false,
        }
    }

    /// Create a new control input port
    pub fn control_input(node_id: NodeId, index: u16, name: &str) -> Self {
        Self {
            id: PortId::control_in(node_id, index),
            name: name.to_string(),
            direction: PortDirection::Input,
            action: None,
            pending_command: None,
            buffer: FixedBuffer::new(),
            feedback_buffer: None,
            downstream: Vec::new(),
            feedback_downstream: Vec::new(),
            feedback_ptrs: Vec::new(),
            downstream_input_ptrs: Vec::new(),
            downstream_nodes: Vec::new(),
            upstream_buffer: None,
            upstream_node: std::ptr::null_mut(),
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
    ///
    /// Nodes should prefer [`read`](Self::read): for a signal **input** this
    /// returns the port's own buffer, which is *not* the effective input on a
    /// zero-copy edge.
    pub fn buffer(&self) -> &FixedBuffer<T, BUF_SIZE> {
        &self.buffer
    }

    /// Low-level mutable access to the port's own buffer (crate-internal).
    ///
    /// Nodes should prefer [`write`](Self::write) / [`write_from`](Self::write_from).
    pub(crate) fn buffer_mut(&mut self) -> &mut FixedBuffer<T, BUF_SIZE> {
        &mut self.buffer
    }

    /// Read the effective input block for this port (zero-copy aware).
    ///
    /// On an exclusive 1:1 edge this is the upstream output buffer; otherwise
    /// it is the port's own (materialized) buffer. This is the correct way for
    /// a node to read a signal input.
    #[inline]
    pub fn read(&self) -> &[T; BUF_SIZE] {
        self.signal_buffer().as_array()
    }

    /// Mutable access to this port's output block. The canonical way for a
    /// node to write its output samples.
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

    // ========================================================================
    // Graph construction API (used by `GraphBuilder::build`)
    //
    // These wire the immutable topology once at build time. They are not
    // intended for node authors — signal data flows through `read`/`write`.
    // ========================================================================

    /// Downstream signal connections `(target_node, target_port)` (serialization).
    #[inline]
    pub fn downstream(&self) -> &[(usize, usize)] {
        &self.downstream
    }

    /// Feedback edge targets from this output port (serialization).
    #[inline]
    pub fn feedback_downstream(&self) -> &[(usize, usize)] {
        &self.feedback_downstream
    }

    /// Direct pointers to downstream input ports (engine propagation).
    #[inline]
    pub fn downstream_input_ptrs(&self) -> &[*mut Port<T, BUF_SIZE>] {
        &self.downstream_input_ptrs
    }

    /// Unique downstream nodes fed by this output port (engine propagation).
    #[inline]
    pub fn downstream_nodes(&self) -> &[*mut crate::traits::NodeVariant<T, BUF_SIZE>] {
        &self.downstream_nodes
    }

    /// The upstream node feeding this input port (pull model), or null.
    #[inline]
    pub fn upstream_node(&self) -> *mut crate::traits::NodeVariant<T, BUF_SIZE> {
        self.upstream_node
    }

    /// Whether this input port aliases an upstream buffer (exclusive 1:1 edge).
    #[inline]
    pub fn has_upstream_buffer(&self) -> bool {
        self.upstream_buffer.is_some()
    }

    /// Record a downstream signal connection `(target_node, target_port)`.
    #[inline]
    pub fn add_downstream(&mut self, target_node: usize, target_port: usize) {
        self.downstream.push((target_node, target_port));
    }

    /// Record a feedback edge target `(target_node, target_port)`.
    #[inline]
    pub fn add_feedback_downstream(&mut self, target_node: usize, target_port: usize) {
        self.feedback_downstream.push((target_node, target_port));
    }

    /// Append a direct pointer to a downstream input port.
    #[inline]
    pub fn add_downstream_input_ptr(&mut self, ptr: *mut Port<T, BUF_SIZE>) {
        self.downstream_input_ptrs.push(ptr);
    }

    /// Append a downstream node pointer, deduplicating on identity.
    #[inline]
    pub fn add_downstream_node(&mut self, node: *mut crate::traits::NodeVariant<T, BUF_SIZE>) {
        let val = node as usize;
        if !self.downstream_nodes.iter().any(|&p| p as usize == val) {
            self.downstream_nodes.push(node);
        }
    }

    /// Set the pull-model upstream node pointer.
    #[inline]
    pub fn set_upstream_node(&mut self, node: *mut crate::traits::NodeVariant<T, BUF_SIZE>) {
        self.upstream_node = node;
    }

    /// Set the zero-copy upstream buffer alias (exclusive 1:1 edges only).
    #[inline]
    pub fn set_upstream_buffer(&mut self, buffer: Option<*const FixedBuffer<T, BUF_SIZE>>) {
        self.upstream_buffer = buffer;
    }

    /// Allocate this port's feedback buffer (feedback edge endpoint).
    #[inline]
    pub fn init_feedback_buffer(&mut self) {
        self.feedback_buffer = Some(FixedBuffer::new());
    }

    /// Raw pointer to this port's `feedback_buffer`, for wiring `feedback_ptrs`.
    #[inline]
    pub fn feedback_buffer_ptr(&self) -> *mut Option<FixedBuffer<T, BUF_SIZE>> {
        &self.feedback_buffer as *const Option<FixedBuffer<T, BUF_SIZE>>
            as *mut Option<FixedBuffer<T, BUF_SIZE>>
    }

    /// Append a pointer to a downstream input port's feedback buffer.
    #[inline]
    pub fn add_feedback_ptr(&mut self, ptr: *mut Option<FixedBuffer<T, BUF_SIZE>>) {
        self.feedback_ptrs.push(ptr);
    }

    /// Whether this input port is a pure zero-copy passthrough.
    ///
    /// True when it aliases a single upstream output buffer
    /// (`upstream_buffer` is set) and performs no per-port processing —
    /// no `action`, no `pending_command`, and no feedback mixing. Such a
    /// port is never materialized into its own `buffer`: the consumer reads
    /// the upstream buffer directly through [`signal_buffer`](Self::signal_buffer).
    #[inline]
    pub fn is_zero_copy(&self) -> bool {
        self.upstream_buffer.is_some()
            && self.action.is_none()
            && self.pending_command.is_none()
            && self.feedback_buffer.is_none()
    }

    /// Get the effective signal buffer for this port.
    ///
    /// For a zero-copy input port returns the upstream output buffer directly.
    /// Otherwise returns the local buffer (materialized by `run_action` /
    /// feedback `pre_process`).
    #[allow(unsafe_code)]
    pub fn signal_buffer(&self) -> &FixedBuffer<T, BUF_SIZE> {
        match self.upstream_buffer {
            Some(ptr) if self.is_zero_copy() => unsafe { &*ptr },
            _ => &self.buffer,
        }
    }

    /// Pre-process this port before node DSP.
    ///
    /// For input ports on a feedback edge, mixes the delayed feedback
    /// (from `feedback_buffer`) into the current `buffer`.
    pub fn pre_process(&mut self) {
        if let Some(ref fb) = self.feedback_buffer {
            let arr = self.buffer.as_mut_array();
            let fb_arr = fb.as_array();
            let chunks = BUF_SIZE / 4;

            for chunk in 0..chunks {
                let o = chunk * 4;
                let a = ScalarVector4::load(&arr[o..o + 4]);
                let b = ScalarVector4::load(&fb_arr[o..o + 4]);
                a.add(&b).store(&mut arr[o..o + 4]);
            }

            for i in chunks * 4..BUF_SIZE {
                arr[i] += fb_arr[i];
            }
        }
    }

    /// Snapshot the buffer into `feedback_buffer` and propagate to
    /// downstream input ports via `feedback_ptrs`.
    ///
    /// For output ports on a feedback edge, saves the current buffer
    /// so it can be used as delayed feedback in the next block, then
    /// copies it into each target input port's `feedback_buffer`.
    /// No-op when `feedback_buffer` is `None`.
    #[allow(unsafe_code)]
    pub fn snapshot_feedback(&mut self) {
        if let Some(ref mut fb) = self.feedback_buffer {
            fb.copy_from(self.buffer.as_array());
            for &ptr in &self.feedback_ptrs {
                unsafe {
                    if let Some(ref mut target) = *ptr {
                        target.copy_from(fb.as_array());
                    }
                }
            }
        }
    }

    /// Propagate this port's own buffer to all downstream input ports.
    ///
    /// For each downstream input port that is **not** a zero-copy passthrough
    /// (fan-in, feedback, or a port with its own `action`/`pending_command`),
    /// materialize the data into that port's buffer via `run_action`.
    /// Zero-copy passthrough ports are left untouched — their consumer reads
    /// this output buffer directly through [`signal_buffer`](Self::signal_buffer),
    /// so no copy is performed. Then process each downstream node and recurse
    /// through its output ports.
    ///
    /// No heap allocations — `downstream_nodes` is pre‑filled at build time.
    #[deprecated = "Port-level propagation is replaced by ScheduledGraph buffer pool execution"]
    #[allow(unsafe_code)]
    pub fn propagate(
        &self,
        ctx: &RenderContext,
        tick: &crate::time::ClockTick,
    ) -> ProcessResult<()> {
        let buffer = self.buffer.as_array();
        for &ptr in &self.downstream_input_ptrs {
            unsafe {
                let p = &mut *ptr;
                if !p.is_zero_copy() {
                    p.run_action(Some(buffer))?;
                }
                p.data_received = true;
            }
        }
        for &parent in &self.downstream_nodes {
            unsafe {
                let nv = &mut *parent;
                for pi in 0..nv.num_signal_inputs() {
                    if let Some(p) = nv.input_port_mut(pi) {
                        p.pre_process();
                    }
                }
                nv.process_block(ctx, tick)?;
                for po in 0..nv.num_signal_outputs() {
                    if let Some(p) = nv.output_port_mut(po) {
                        p.snapshot_feedback();
                    }
                }
                for po in 0..nv.num_signal_outputs() {
                    if let Some(p) = nv.output_port(po) {
                        p.propagate(ctx, tick)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Run the port's algorithm.
    ///
    /// Delivers any pending command via `Algorithm::apply_command()`, then
    /// calls `Algorithm::process()` with the input and output slices.
    /// When no algorithm is attached, the pending command value (if any)
    /// is written directly into the buffer; otherwise input is passed
    /// through or zero-filled.
    pub fn run_action(
        &mut self,
        input: Option<&[T; BUF_SIZE]>,
    ) -> crate::traits::ProcessResult<()> {
        match &mut self.action {
            Some(action) => {
                // Deliver any pending command to the algorithm
                if let Some(cmd) = self.pending_command.take() {
                    action.apply_command(cmd);
                }
                let input_slice = input.map(|arr| arr.as_slice());
                action.process(input_slice, self.buffer.as_mut_slice())
            }
            None => {
                // No algorithm — use pending command value if set,
                // otherwise pass through input or zero-fill.
                if let Some(cmd) = self.pending_command.take() {
                    self.buffer.fill(cmd);
                } else if let Some(input_data) = input {
                    self.buffer.copy_from(input_data);
                } else {
                    self.buffer.fill(T::ZERO);
                }
                Ok(())
            }
        }
    }

    /// Set a command value for this port.
    ///
    /// The value is stored as a pending command and delivered to the
    /// algorithm (or written directly to the buffer) on the next
    /// `run_action()` call.
    pub fn set_value(&mut self, value: T) {
        self.pending_command = Some(value);
    }
}

// ============================================================================
// Send / Sync
// ============================================================================

// SAFETY: `upstream_buffer` is a raw pointer to a buffer owned by another
// Port in the same static graph. The graph is immutable during processing
// and runs single-threaded in topological order. The pointer target
// outlives the pointer for the entire processing session.
#[allow(unsafe_code)]
unsafe impl<T: Transcendental + Send, const BUF_SIZE: usize> Send for Port<T, BUF_SIZE> {}
#[allow(unsafe_code)]
unsafe impl<T: Transcendental + Sync, const BUF_SIZE: usize> Sync for Port<T, BUF_SIZE> {}

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
