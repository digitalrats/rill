//! Rack module type definitions.
//!
//! Always compiled. Serialisation derives are conditional on the
//! `serde` feature.

#![allow(missing_docs)]

use std::collections::HashMap;

use rill_core::traits::ParamValue;

use crate::automaton::envelope::EnvelopeType;
use crate::automaton::lfo::LfoWaveform;
use crate::automaton::sequencer::PlayMode;
use crate::engine::{ParameterMapping, Transform};
use crate::strategy::{ConflictStrategy, ControlStrategy};

// ============================================================================
// AutomatonDef
// ============================================================================

/// Serializable description of a control automaton.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum AutomatonDef {
    Lfo {
        id: String,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
    },
    Envelope {
        id: String,
        envelope_type: EnvelopeType,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        curve: f64,
    },
    Sequencer {
        id: String,
        steps: Vec<StepDef>,
        play_mode: PlayMode,
        tempo: f64,
    },
    NamedFunction {
        id: String,
        function_name: String,
        params: HashMap<String, f64>,
    },
    /// Custom automaton — dispatched via [`AutomatonFactory`].
    Custom {
        id: String,
        type_name: String,
        #[cfg_attr(feature = "serde", serde(default))]
        params: HashMap<String, ParamValue>,
    },
}

impl AutomatonDef {
    pub fn id(&self) -> &str {
        match self {
            AutomatonDef::Lfo { id, .. } => id,
            AutomatonDef::Envelope { id, .. } => id,
            AutomatonDef::Sequencer { id, .. } => id,
            AutomatonDef::NamedFunction { id, .. } => id,
            AutomatonDef::Custom { id, .. } => id,
        }
    }
}

/// Serializable step for [`AutomatonDef::Sequencer`].
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct StepDef {
    /// Duration in beat fractions (1.0 = quarter note at the given tempo).
    pub duration: f64,
}

// ============================================================================
// ServoDef
// ============================================================================

/// Type of value mapping for a servo.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MappingType {
    Linear,
    Exponential,
    Logarithmic,
    Inverted,
}

impl MappingType {
    pub fn to_parameter_mapping(self) -> ParameterMapping {
        match self {
            MappingType::Linear => ParameterMapping::Linear,
            MappingType::Exponential => ParameterMapping::Exponential,
            MappingType::Logarithmic => ParameterMapping::Logarithmic,
            MappingType::Inverted => ParameterMapping::Inverted,
        }
    }
}

/// Describes a servo: which automaton drives which node parameter.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct ServoDef {
    pub automaton_id: String,
    pub target_node: u32,
    pub target_param: String,
    pub mapping: MappingType,
    pub min: f64,
    pub max: f64,
    pub enabled: bool,

    /// Async mode: update interval in milliseconds.
    /// When `Some`, the automaton runs as a green thread (tokio task)
    /// with the given interval. When `None`, falls back to sync mode
    /// (requires manual `Patchbay::update()` calls).
    #[cfg_attr(feature = "serde", serde(default))]
    pub async_interval_ms: Option<f64>,

    /// Async mode: control strategy (defaults to `Absolute`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub control_strategy: Option<ControlStrategy>,

    /// Async mode: conflict resolution (defaults to `LastWriteWins`).
    #[cfg_attr(feature = "serde", serde(default))]
    pub conflict_strategy: Option<ConflictStrategy>,

    /// Optional value table for index-based automatons.
    /// When set, the servo looks up `table[automaton_output]`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub table: Option<Vec<ParamValue>>,

    /// String anchor name for rill-lang graph nodes.
    /// When set, the servo sends `GraphSetParameter` to the
    /// RillGraphEngine using this anchor instead of a `PortId`.
    #[cfg_attr(feature = "serde", serde(default))]
    pub target_anchor: Option<String>,
}

// ============================================================================
// MappingDef
// ============================================================================

/// Serializable transform — Linear, Exponential, Logarithmic, or Inverted.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum TransformDef {
    Linear,
    Exponential,
    Logarithmic,
    Inverted,
}

impl TransformDef {
    pub fn to_transform(&self) -> Transform {
        match self {
            TransformDef::Linear => Transform::Linear,
            TransformDef::Exponential => Transform::Exponential,
            TransformDef::Logarithmic => Transform::Logarithmic,
            TransformDef::Inverted => Transform::Inverted,
        }
    }
}

/// Describes a mapping from an external event to a node parameter.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct MappingDef {
    pub event_pattern: crate::engine::EventPattern,
    pub target_node: u32,
    pub target_param: String,
    pub transform: TransformDef,
    pub min: f64,
    pub max: f64,
    pub enabled: bool,
}

impl MappingDef {
    pub fn to_mapping(&self) -> crate::engine::Mapping {
        use crate::engine::Target;
        crate::engine::Mapping::new(
            self.event_pattern.clone(),
            Target {
                node_id: rill_core::traits::NodeId(self.target_node),
                param_name: self.target_param.clone(),
                min: self.min as f32,
                max: self.max as f32,
            },
            self.transform.to_transform(),
        )
    }
}

// ============================================================================
// SensorDef
// ============================================================================

/// Serializable external input sensor.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum SensorDef {
    /// MIDI input.
    Midi {
        /// Backend type — `"midir"` or `"alsa_seq"`.
        backend: String,
        /// Port name for the backend.
        port_name: String,
        /// Event-to-parameter mappings (CC → param, Note → param, etc.).
        #[cfg_attr(feature = "serde", serde(default))]
        mappings: Vec<MappingDef>,
    },
    /// OSC input over UDP.
    Osc {
        /// UDP port to listen on.
        port: u16,
        /// Event-to-parameter mappings (OSC address → param).
        #[cfg_attr(feature = "serde", serde(default))]
        mappings: Vec<MappingDef>,
    },
}

impl SensorDef {
    /// Returns the event-to-parameter mappings, if any.
    pub fn get_mappings(&self) -> Vec<crate::engine::Mapping> {
        match self {
            SensorDef::Midi { mappings, .. } => mappings.iter().map(|m| m.to_mapping()).collect(),
            SensorDef::Osc { mappings, .. } => mappings.iter().map(|m| m.to_mapping()).collect(),
        }
    }

    #[cfg(any(feature = "midi", feature = "osc"))]
    pub fn into_sensor(&self) -> Option<Box<dyn crate::sensor::Sensor>> {
        match self {
            #[cfg(feature = "midi")]
            SensorDef::Midi {
                backend,
                port_name,
                mappings: _,
            } => {
                use rill_io::midi_input::MidiInput;
                let be: Box<dyn MidiInput> = match backend.as_str() {
                    "midir" => Box::new(rill_io::backends::MidirBackend::new(port_name).ok()?),
                    "alsa_seq" => {
                        #[cfg(feature = "alsa")]
                        {
                            Box::new(
                                rill_io::backends::AlsaSeqBackend::new(port_name)
                                    .map_err(|e| log::warn!("AlsaSeqBackend: {e}"))
                                    .ok()?,
                            )
                        }
                        #[cfg(not(feature = "alsa"))]
                        {
                            log::warn!("ALSA seq backend requires 'alsa' feature");
                            return None;
                        }
                    }
                    _ => {
                        log::warn!("unknown MIDI backend '{backend}'");
                        return None;
                    }
                };
                let hub = crate::midi::MidiHub::new(port_name.as_str(), be);
                Some(Box::new(hub))
            }
            #[cfg(feature = "osc")]
            SensorDef::Osc { port, mappings: _ } => {
                let addr = std::net::SocketAddr::from(([0, 0, 0, 0], *port));
                let sensor = crate::osc::OscSensor::new(format!("osc_{port}"), addr);
                Some(Box::new(sensor))
            }
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }
    #[cfg(not(any(feature = "midi", feature = "osc")))]
    pub fn into_sensor(&self) -> Option<Box<dyn crate::sensor::Sensor>> {
        None
    }
}

// ============================================================================
// ClockDef — MIDI clock output definition
// ============================================================================

/// Serializable MIDI clock output configuration.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct ClockDef {
    /// Backend type — `"midir"`, `"alsa_seq"`, or `"jack"`.
    pub backend: String,
    /// Port name for the backend.
    pub port_name: String,
    /// Start clock automatically when the system launches.
    #[cfg_attr(feature = "serde", serde(default))]
    pub auto_start: bool,
}

// ============================================================================
// ModuleDef — unified servo, sensor, and custom module serialization
// ============================================================================

/// A rack module — either a Servo (automaton → parameter), a Sensor (external input),
/// or a Custom module dispatched through [`ModuleFactory`](crate::module_factory::ModuleFactory).
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub enum ModuleDef {
    /// MIDI clock output module.
    Clock(ClockDef),
    /// Servo: automaton → graph parameter bridge.
    Servo(ServoDef),
    /// Sensor: external input (MIDI, OSC, etc.).
    Sensor(SensorDef),
    /// Custom module — dispatched through the module factory.
    Custom {
        /// Module type name for factory lookup.
        type_name: String,
        /// Module-specific parameters.
        #[cfg_attr(feature = "serde", serde(default))]
        params: HashMap<String, ParamValue>,
    },
}

impl ModuleDef {
    /// Returns the factory registration key for this module.
    pub fn type_name(&self) -> &str {
        match self {
            ModuleDef::Clock(_) => "clock",
            ModuleDef::Servo(_) => "servo",
            ModuleDef::Sensor(SensorDef::Midi { .. }) => "midi",
            ModuleDef::Sensor(SensorDef::Osc { .. }) => "osc",
            ModuleDef::Custom { type_name, .. } => type_name,
        }
    }
}
