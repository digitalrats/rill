use rill_core::NodeId;

/// A single parameter target within a snapshot or step.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterTarget {
    /// Graph node ID.
    pub node_id: NodeId,
    /// Parameter name on the target node.
    pub param_name: String,
    /// Stored value for this parameter.
    pub value: f32,
}

impl ParameterTarget {
    /// Create a new parameter target.
    pub fn new(node_id: NodeId, param_name: impl Into<String>, value: f32) -> Self {
        Self {
            node_id,
            param_name: param_name.into(),
            value,
        }
    }
}

/// A named collection of parameter values — a complete preset "scene".
///
/// Snapshots are a convenience for storing/recalling complete parameter sets.
/// A [`SequenceStep`](super::SequenceStep) expands its referenced snapshot's
/// parameters into the step's own lock list on step advance.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Unique snapshot identifier.
    pub id: String,
    /// Parameter targets stored in this snapshot.
    pub parameters: Vec<ParameterTarget>,
}

impl Snapshot {
    /// Create a new snapshot with the given ID and parameters.
    pub fn new(id: impl Into<String>, parameters: Vec<ParameterTarget>) -> Self {
        Self {
            id: id.into(),
            parameters,
        }
    }
}
