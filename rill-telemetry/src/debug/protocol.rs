//! Protocol types for the analyzer ↔ collector thread communication.

use std::path::PathBuf;

use rill_lang::ir::ProbeId;
use serde::{Deserialize, Serialize};

/// Commands sent from the analyzer to the collector thread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnalyzerCommand {
    /// Set a breakpoint at the given probe.
    SetBreakpoint {
        /// The probe to break on.
        probe_id: ProbeId,
    },
    /// Clear a breakpoint at the given probe.
    ClearBreakpoint {
        /// The probe to clear.
        probe_id: ProbeId,
    },
    /// Continue execution after a pause.
    Continue,
    /// Step one block and pause again.
    Step,
    /// Get the latest value of a specific probe.
    GetProbeValue {
        /// The probe to read.
        probe_id: ProbeId,
    },
    /// Get the latest values of all enabled probes.
    GetProbeValues,
    /// List all graph nodes.
    ListNodes,
    /// List all probes.
    ListProbes,
    /// List captured command frames.
    ListCommands,
    /// Enable a probe.
    EnableProbe {
        /// The probe to enable.
        probe_id: ProbeId,
    },
    /// Disable a probe.
    DisableProbe {
        /// The probe to disable.
        probe_id: ProbeId,
    },
    /// Pause the engine.
    Pause,
    /// List all registered automatons.
    ListAutomatons,
    /// Get the state of a specific automaton.
    GetAutomatonState { name: String },
    /// List all sensors.
    ListSensors,
    /// Get the status of a specific sensor.
    GetSensorStatus { name: String },
    /// List all queues.
    ListQueues,
    /// Shut down the collector thread.
    Quit,
}

/// Responses sent from the collector thread back to the analyzer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AnalyzerResponse {
    /// Generic success acknowledgment.
    Ok,
    /// Value of a single probe.
    ProbeValue {
        /// Probe identifier.
        probe_id: ProbeId,
        /// Raw value bits.
        value_bits: u64,
    },
    /// Values of all enabled probes.
    ProbeValues(Vec<(ProbeId, u64)>),
    /// List of graph nodes.
    NodeList(Vec<NodeInfo>),
    /// List of probes.
    ProbeList(Vec<ProbeInfo>),
    /// Command log entries.
    CommandLog(Vec<CommandLogEntry>),
    /// An error occurred.
    Error(String),
    /// List of automaton names.
    AutomatonsList(Vec<String>),
    /// State of a single automaton.
    AutomatonState(String),
    /// List of sensor names.
    SensorList(Vec<String>),
    /// Status of a single sensor.
    SensorStatus(String),
    /// List of queue statistics.
    QueueList(Vec<QueueStats>),
    /// Engine is now paused.
    Paused,
}

/// Information about a graph node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Node name.
    pub name: String,
    /// Node type description.
    pub node_type: String,
    /// Number of signal inputs.
    pub num_inputs: usize,
    /// Number of signal outputs.
    pub num_outputs: usize,
}

/// Information about a probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeInfo {
    /// Unique probe identifier.
    pub probe_id: ProbeId,
    /// Human-readable probe name.
    pub name: String,
    /// Name of the node containing this probe.
    pub node_name: String,
    /// Whether the probe is enabled.
    pub enabled: bool,
    /// Whether this probe has a breakpoint set.
    pub has_breakpoint: bool,
}

/// A single command log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandLogEntry {
    /// Block index when the command was received.
    pub block_index: u64,
    /// Command kind label.
    pub command_kind: String,
    /// Target node name.
    pub node_name: String,
    /// Optional parameter name.
    pub param_name: Option<String>,
    /// String representation of the value.
    pub value_repr: String,
}

/// Analyzer output configuration.
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    /// Output format mode.
    pub output: OutputMode,
    /// Optional log file path.
    pub log_file: Option<PathBuf>,
}

impl Default for AnalyzerConfig {
    fn default() -> Self {
        Self {
            output: OutputMode::Text,
            log_file: None,
        }
    }
}

/// Output rendering mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputMode {
    /// Human-readable colored text output.
    Text,
    /// JSON Lines format.
    Json,
}

/// Statistics about a named queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStats {
    /// Queue name.
    pub name: String,
    /// Maximum capacity of the queue.
    pub capacity: usize,
    /// Current number of items in the queue.
    pub len: usize,
    /// Whether the queue is at capacity.
    pub is_full: bool,
}
