#![allow(missing_docs)]
//! Serializable rack document types (de)serialised from JSON/CBOR.

pub use crate::engine::EventPattern;
use crate::engine::OscSurface;

// Re-export all module definition types from the always-compiled module.
pub use crate::module_def::{
    AutomatonDef, ClockDef, MappingDef, MappingType, ModuleDef, SensorDef, ServoDef, StepDef,
    TransformDef,
};

// ============================================================================
// PatchbayDef
// ============================================================================

/// Serializable patchbay configuration — automatons + modules without a signal graph.
/// For full rack configuration (graph + automatons + modules), use
/// [`rill_adrift::modular::serialization::RackDef`].
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct PatchbayDef {
    pub automatons: Vec<AutomatonDef>,
    /// Unified modules — servos and sensors.
    pub modules: Vec<ModuleDef>,
    pub mappings: Vec<MappingDef>,

    /// OSC → EventPattern bridge.
    #[serde(default)]
    pub osc_surface: OscSurface,

    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl PatchbayDef {
    pub fn new() -> Self {
        Self {
            automatons: Vec::new(),
            modules: Vec::new(),
            mappings: Vec::new(),
            osc_surface: Vec::new(),
            description: None,
        }
    }
}

impl Default for PatchbayDef {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Serialisation helpers
// ============================================================================

#[cfg(feature = "json")]
pub fn to_json(doc: &PatchbayDef) -> Result<String, String> {
    serde_json::to_string_pretty(doc).map_err(|e| e.to_string())
}

#[cfg(feature = "json")]
pub fn from_json(json: &str) -> Result<PatchbayDef, String> {
    serde_json::from_str(json).map_err(|e| e.to_string())
}

#[cfg(feature = "cbor")]
pub fn to_cbor(doc: &PatchbayDef) -> Result<Vec<u8>, String> {
    serde_cbor::to_vec(doc).map_err(|e| e.to_string())
}

#[cfg(feature = "cbor")]
pub fn from_cbor(bytes: &[u8]) -> Result<PatchbayDef, String> {
    serde_cbor::from_slice(bytes).map_err(|e| e.to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automaton::lfo::LfoWaveform;

    fn sample_doc() -> PatchbayDef {
        PatchbayDef {
            automatons: vec![AutomatonDef::Lfo {
                id: "lfo1".into(),
                frequency: 0.3,
                amplitude: 1.0,
                offset: 0.0,
                waveform: LfoWaveform::Sine,
            }],
            modules: vec![ModuleDef::Servo(ServoDef {
                automaton_id: "lfo1".into(),
                target_node: 1,
                target_param: "delay_time".into(),
                mapping: MappingType::Linear,
                min: 0.01,
                max: 0.5,
                enabled: true,
                async_interval_ms: None,
                control_strategy: None,
                conflict_strategy: None,
                table: None,
            })],
            mappings: vec![],
            osc_surface: vec![],
            description: None,
        }
    }

    #[test]
    #[cfg(feature = "json")]
    fn test_json_roundtrip() {
        let doc = sample_doc();
        let json = to_json(&doc).unwrap();
        let restored = from_json(&json).unwrap();
        assert_eq!(restored.automatons.len(), 1);
        assert_eq!(restored.modules.len(), 1);
        match &restored.modules[0] {
            ModuleDef::Servo(s) => assert_eq!(s.target_param, "delay_time"),
            _ => panic!("expected Servo"),
        }
    }

    #[test]
    #[cfg(feature = "cbor")]
    fn test_cbor_roundtrip() {
        let doc = sample_doc();
        let cbor = to_cbor(&doc).unwrap();
        let restored = from_cbor(&cbor).unwrap();
        assert_eq!(restored.automatons.len(), 1);
        assert_eq!(restored.automatons[0].id(), "lfo1");
    }
}
