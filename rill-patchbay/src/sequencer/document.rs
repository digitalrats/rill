//! # Sequencer serialisation
//!
//! [`SequencerDocument`] is a standalone serialisable description of a
//! parameter-lock sequencer configuration.  It is independent from
//! [`PatchbayDocument`](crate::document::PatchbayDocument) — you can
//! load, save, and swap sequencer presets separately from LFO/envelope
//! automation and event mappings.
//!
//! ## Feature gate
//!
//! This module is available only when the `serde` feature is enabled.

use super::{Pattern, Snapshot, SnapshotSequencer};

/// Serializable snapshot-sequencer configuration.
///
/// Contains everything needed to reconstruct a running sequencer:
/// named snapshots (preset parameter sets), patterns (step sequences),
/// active pattern selection, and auto-start behaviour.
///
/// # Round-trip safety
///
/// The optional `description` field is preserved through serialisation
/// roundtrips but never interpreted by the engine.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct SequencerDocument {
    /// Named parameter snapshots (collections of p-locks).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snapshots: Vec<Snapshot>,

    /// Named step patterns.
    pub patterns: Vec<Pattern>,

    /// Active pattern ID (empty = none — first pattern used as default).
    #[serde(default)]
    pub active_pattern: String,

    /// Whether to start the sequencer immediately on load.
    #[serde(default)]
    pub auto_start: bool,

    /// Optional human-readable description (attribution, notes, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl SequencerDocument {
    /// Create a document with the given patterns and no snapshots.
    pub fn new(patterns: Vec<Pattern>) -> Self {
        Self {
            snapshots: Vec::new(),
            patterns,
            active_pattern: String::new(),
            auto_start: false,
            description: None,
        }
    }

    /// Build a [`SnapshotSequencer`] from this document, applying the
    /// active-pattern and auto-start settings.
    pub fn into_sequencer(self) -> SnapshotSequencer {
        let mut seq = SnapshotSequencer::with_lib(self.snapshots, self.patterns);
        if !self.active_pattern.is_empty() {
            seq.set_active_pattern(&self.active_pattern);
        }
        if self.auto_start {
            seq.start();
        }
        seq
    }

    /// Serialise to pretty-printed JSON.
    #[cfg(feature = "json")]
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    /// Deserialise from a JSON string.
    #[cfg(feature = "json")]
    pub fn from_json(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    /// Serialise to CBOR binary.
    #[cfg(feature = "cbor")]
    pub fn to_cbor(&self) -> Result<Vec<u8>, String> {
        serde_cbor::to_vec(self).map_err(|e| e.to_string())
    }

    /// Deserialise from CBOR binary.
    #[cfg(feature = "cbor")]
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, String> {
        serde_cbor::from_slice(bytes).map_err(|e| e.to_string())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
#[cfg(feature = "serde")]
mod tests {
    use super::*;
    use crate::sequencer::{ParameterTarget, SequenceStep, StepPlayMode};
    use rill_core::NodeId;

    fn sample_doc() -> SequencerDocument {
        SequencerDocument {
            snapshots: vec![Snapshot::new(
                "verse",
                vec![ParameterTarget::new(NodeId(1), "gain", 0.8)],
            )],
            patterns: vec![Pattern::new(
                "main",
                vec![
                    SequenceStep::single(NodeId(1), "cutoff", 0.3, 1.0),
                    SequenceStep::single(NodeId(1), "cutoff", 0.7, 1.0),
                ],
            )],
            active_pattern: "main".into(),
            auto_start: true,
            description: Some("test preset".into()),
        }
    }

    #[test]
    fn test_json_roundtrip() {
        let doc = sample_doc();
        let json = doc.to_json().unwrap();
        let restored = SequencerDocument::from_json(&json).unwrap();

        assert_eq!(restored.snapshots.len(), 1);
        assert_eq!(restored.patterns.len(), 1);
        assert_eq!(restored.active_pattern, "main");
        assert!(restored.auto_start);
        assert_eq!(restored.description.as_deref(), Some("test preset"));
        assert_eq!(restored.patterns[0].steps.len(), 2);
    }

    #[test]
    fn test_into_sequencer() {
        let doc = sample_doc();
        let seq = doc.into_sequencer();
        assert!(seq.is_running());
        assert_eq!(seq.active_pattern(), "main");
    }

    #[test]
    fn test_empty_document() {
        let doc = SequencerDocument::new(vec![]);
        let json = doc.to_json().unwrap();
        let restored = SequencerDocument::from_json(&json).unwrap();
        assert!(restored.patterns.is_empty());
        assert!(restored.snapshots.is_empty());

        let seq = restored.into_sequencer();
        assert!(!seq.is_running());
    }

    #[cfg(feature = "cbor")]
    #[test]
    fn test_cbor_roundtrip() {
        let doc = sample_doc();
        let cbor = doc.to_cbor().unwrap();
        let restored = SequencerDocument::from_cbor(&cbor).unwrap();
        assert_eq!(restored.active_pattern, "main");
    }
}
