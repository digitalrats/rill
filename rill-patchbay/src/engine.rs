//! Control and automation subsystem.
//!
//! Provides event mapping (MIDI/OSC → parameters), automaton-based
//! modulation (LFO, envelopes), and a two-thread model with lock-free
//! queues for control → audio communication.

use std::fmt::Debug;
use std::sync::{Arc, Mutex};

use rill_core::prelude::*;
use rill_core::queues::{AutomatonCommand, CommandEnum, SetParameter, SignalOrigin};
use rill_core_actor::{ActorRef, ActorSystem};

pub use crate::automaton::{EnvelopeAutomaton, LfoAutomaton, LfoWaveform, Range};
use crate::strategy::{ConflictStrategy, ControlStrategy};

// =============================================================================
// 1. Event patterns
// =============================================================================

/// A pattern for matching controller events.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventPattern {
    AnyButton,
    ButtonId(u32),
    AnyKnob,
    KnobId(u32),
    AnyFader,
    FaderId(u32),
    AnyMidi,
    MidiControl {
        channel: Option<u8>,
        controller: u8,
    },
    MidiNote {
        channel: Option<u8>,
        note: Option<u8>,
    },
    MidiClock,
    MidiTransport {
        kind: Option<MidiTransportKind>,
    },
    OscAddress(String),
    OscPattern(String),
}

impl EventPattern {
    pub fn matches(&self, event: &ControlEvent) -> bool {
        match (self, event) {
            (EventPattern::AnyButton, ControlEvent::Button { .. }) => true,
            (EventPattern::ButtonId(id), ControlEvent::Button { id: eid, .. }) => *id == *eid,
            (EventPattern::AnyKnob, ControlEvent::Knob { .. }) => true,
            (EventPattern::KnobId(id), ControlEvent::Knob { id: eid, .. }) => *id == *eid,
            (EventPattern::AnyFader, ControlEvent::Fader { .. }) => true,
            (EventPattern::FaderId(id), ControlEvent::Fader { id: eid, .. }) => *id == *eid,
            (
                EventPattern::MidiControl {
                    channel,
                    controller,
                },
                ControlEvent::MidiControl {
                    channel: ech,
                    controller: ectr,
                    ..
                },
            ) => (channel.is_none() || channel.unwrap() == *ech) && *controller == *ectr,
            (
                EventPattern::MidiNote { channel, note },
                ControlEvent::MidiNote {
                    channel: ech,
                    note: en,
                    ..
                },
            ) => {
                (channel.is_none() || channel.unwrap() == *ech)
                    && (note.is_none() || note.unwrap() == *en)
            }
            (EventPattern::AnyMidi, ControlEvent::MidiControl { .. })
            | (EventPattern::AnyMidi, ControlEvent::MidiNote { .. })
            | (EventPattern::AnyMidi, ControlEvent::MidiClock)
            | (EventPattern::AnyMidi, ControlEvent::MidiTransport { .. }) => true,
            (EventPattern::MidiClock, ControlEvent::MidiClock) => true,
            (
                EventPattern::MidiTransport { kind },
                ControlEvent::MidiTransport { kind: ek, .. },
            ) => kind.is_none_or(|k| k == *ek),
            (EventPattern::OscAddress(addr), ControlEvent::Osc { address, .. }) => addr == address,
            (EventPattern::OscPattern(pat), ControlEvent::Osc { address, .. }) => {
                address.contains(pat)
            }
            _ => false,
        }
    }
}

// =============================================================================
// 2. Event types
// =============================================================================

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum ControlEvent {
    Button {
        id: u32,
        pressed: bool,
    },
    Knob {
        id: u32,
        value: f32,
        normalized: f32,
    },
    Fader {
        id: u32,
        value: f32,
        normalized: f32,
    },
    MidiControl {
        channel: u8,
        controller: u8,
        value: u8,
        normalized: f32,
    },
    MidiNote {
        channel: u8,
        note: u8,
        velocity: u8,
        on: bool,
    },
    Osc {
        address: String,
        args: Vec<f32>,
    },
    MidiClock,
    MidiTransport {
        kind: MidiTransportKind,
    },
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MidiTransportKind {
    Start,
    Stop,
    Continue,
}

impl ControlEvent {
    pub fn normalized_value(&self) -> Option<f32> {
        match self {
            ControlEvent::Knob { normalized, .. } => Some(*normalized),
            ControlEvent::Fader { normalized, .. } => Some(*normalized),
            ControlEvent::MidiControl { normalized, .. } => Some(*normalized),
            ControlEvent::Button { pressed, .. } => Some(if *pressed { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    pub fn id(&self) -> Option<u32> {
        match self {
            ControlEvent::Button { id, .. } => Some(*id),
            ControlEvent::Knob { id, .. } => Some(*id),
            ControlEvent::Fader { id, .. } => Some(*id),
            _ => None,
        }
    }
}

// =============================================================================
// 2b. OSC Surface
// =============================================================================

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct OscSurfaceEntry {
    pub osc_path: String,
    pub event_pattern: EventPattern,
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub label: Option<String>,
}

pub type OscSurface = Vec<OscSurfaceEntry>;

// =============================================================================
// 3. Value transforms
// =============================================================================

#[derive(Clone)]
pub enum Transform {
    Linear,
    Exponential,
    Logarithmic,
    Inverted,
    Custom(Arc<dyn Fn(f32) -> f32 + Send + Sync>),
}

impl Debug for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transform::Linear => write!(f, "Linear"),
            Transform::Exponential => write!(f, "Exponential"),
            Transform::Logarithmic => write!(f, "Logarithmic"),
            Transform::Inverted => write!(f, "Inverted"),
            Transform::Custom(_) => write!(f, "Custom"),
        }
    }
}

impl Transform {
    pub fn apply(&self, value: f32, min: f32, max: f32) -> f32 {
        let range = max - min;
        let normalized = value.clamp(0.0, 1.0);
        let mapped = match self {
            Transform::Linear => min + normalized * range,
            Transform::Exponential => min + normalized * normalized * range,
            Transform::Logarithmic => min + (1.0 + normalized * 9.0).log10() * range,
            Transform::Inverted => max - normalized * range,
            Transform::Custom(f) => min + f(normalized) * range,
        };
        mapped.clamp(min, max)
    }
}

// =============================================================================
// 4. Event mapping
// =============================================================================

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Target {
    pub node_id: NodeId,
    pub param_name: String,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone)]
pub struct Mapping {
    pub pattern: EventPattern,
    pub target: Target,
    pub transform: Transform,
    pub name: String,
    pub enabled: bool,
}

impl Mapping {
    pub fn new(pattern: EventPattern, target: Target, transform: Transform) -> Self {
        let name = format!("{:?} -> {}", pattern, target.param_name);
        Self {
            pattern,
            target,
            transform,
            name,
            enabled: true,
        }
    }

    pub fn matches(&self, event: &ControlEvent) -> bool {
        self.enabled && self.pattern.matches(event)
    }

    pub fn apply(&self, event: &ControlEvent) -> Option<SetParameter> {
        if !self.matches(event) {
            return None;
        }
        event.normalized_value().map(|norm| {
            let value = self.transform.apply(norm, self.target.min, self.target.max);
            let pid = ParameterId::new(&self.target.param_name).unwrap();
            SetParameter::new(
                PortId::param(self.target.node_id, 0),
                pid,
                ParamValue::Float(value),
                SignalOrigin::External(self.name.clone()),
            )
        })
    }
}

// =============================================================================
// 5. Automaton core trait
// =============================================================================

pub type Time = f64;

#[derive(Debug, Clone, Default)]
pub struct NoAction;

pub trait Automaton: Send + Sync + Debug {
    type Internal: Clone + Send + Sync + 'static;
    type Action: Debug + Clone + Send + Sync + Default + 'static;

    fn step(
        &self,
        internal: &mut Self::Internal,
        current: &ParamValue,
        time: Time,
        action: &Self::Action,
    ) -> ParamValue;

    fn initial_internal(&self) -> Self::Internal;

    fn reset(&self) -> Self::Internal {
        self.initial_internal()
    }

    fn name(&self) -> &str;
}

// =============================================================================
// 6. Parameter mapping
// =============================================================================

#[derive(Clone)]
pub enum ParameterMapping {
    Linear,
    Exponential,
    Logarithmic,
    Inverted,
    Custom(Arc<dyn Fn(f64) -> f64 + Send + Sync>),
}

impl std::fmt::Debug for ParameterMapping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParameterMapping::Linear => write!(f, "Linear"),
            ParameterMapping::Exponential => write!(f, "Exponential"),
            ParameterMapping::Logarithmic => write!(f, "Logarithmic"),
            ParameterMapping::Inverted => write!(f, "Inverted"),
            ParameterMapping::Custom(_) => write!(f, "Custom(<fn>)"),
        }
    }
}

impl ParameterMapping {
    pub fn apply(&self, raw: f64) -> f64 {
        match self {
            ParameterMapping::Linear => raw,
            ParameterMapping::Exponential => raw * raw,
            ParameterMapping::Logarithmic => (1.0 + raw * 9.0).log10(),
            ParameterMapping::Inverted => 1.0 - raw,
            ParameterMapping::Custom(f) => f(raw),
        }
    }
}

// =============================================================================
// 7. ServoState
// =============================================================================

pub(crate) struct ServoState<A: Automaton> {
    pub(crate) internal: A::Internal,
    pub(crate) value: ParamValue,
    pub(crate) time: Time,
    pub(crate) enabled: bool,
    pub(crate) base: f64,
    pub(crate) frozen: bool,
    pub(crate) last_sent_value: f64,
    pub(crate) last_sent_index: i64,
}

// =============================================================================
// 8. Servo — automaton-to-parameter bridge
// =============================================================================

pub struct Servo<A: Automaton> {
    id: String,
    automaton: Arc<A>,
    state: Arc<Mutex<ServoState<A>>>,
    graph_ref: ActorRef<CommandEnum>,
    target_node: NodeId,
    target_param: String,
    mapping: ParameterMapping,
    min: f64,
    max: f64,
    control: ControlStrategy,
    conflict: ConflictStrategy,
    table: Option<Vec<ParamValue>>,
}

impl<A: Automaton + 'static> Servo<A> {
    pub fn new(
        id: impl Into<String>,
        automaton: A,
        target_node: NodeId,
        target_param: impl Into<String>,
        mapping: ParameterMapping,
        min: f64,
        max: f64,
        system: Arc<ActorSystem>,
        graph_ref: ActorRef<CommandEnum>,
    ) -> Self {
        let _ = system;
        let automaton = Arc::new(automaton);
        let mut internal = automaton.initial_internal();
        let initial_value = automaton.step(
            &mut internal,
            &ParamValue::Float(0.0),
            0.0,
            &A::Action::default(),
        );

        Self {
            id: id.into(),
            automaton,
            state: Arc::new(Mutex::new(ServoState {
                internal,
                value: initial_value,
                time: 0.0,
                enabled: true,
                base: (min + max) / 2.0,
                frozen: false,
                last_sent_value: f64::NAN,
                last_sent_index: -1,
            })),
            graph_ref,
            target_node,
            target_param: target_param.into(),
            mapping,
            min,
            max,
            control: ControlStrategy::Absolute,
            conflict: ConflictStrategy::LastWriteWins,
            table: None,
        }
    }

    pub fn spawn(self, system: &ActorSystem) -> ActorRef<CommandEnum> {
        let Servo {
            id,
            automaton,
            state,
            graph_ref,
            target_node,
            target_param,
            mapping,
            min,
            max,
            control,
            conflict,
            table,
        } = self;

        let a = automaton;
        let s = state;
        let gr = graph_ref;
        let nid = target_node;
        let param = target_param;
        let map = mapping;
        let ctrl = control;
        let confl = conflict;
        let tbl = table;
        let serv_id = id.clone();

        let s2 = s.clone();
        system.spawn_detached_tokio(
            &format!("servo_{id}"),
            move || {
                Box::new(move |msg: CommandEnum| match msg {
                    CommandEnum::ClockTick(clock) => {
                        let mut state = s2.lock().unwrap();
                        if !state.enabled {
                            return;
                        }
                        let dt = clock.samples_since_last as f64 / clock.sample_rate as f64;
                        state.time += dt;
                        if state.frozen && matches!(confl, ConflictStrategy::TouchOverride) {
                            return;
                        }
                        let current_value = state.value.clone();
                        let current_time = state.time;
                        let action = A::Action::default();
                        let new_val =
                            a.step(&mut state.internal, &current_value, current_time, &action);
                        let raw = new_val.as_f32().unwrap_or(0.0) as f64;
                        state.value = new_val;

                        if let Some(ref table) = tbl {
                            let index = raw as usize;
                            if index >= table.len() {
                                return;
                            }
                            let idx = index as i64;
                            if idx == state.last_sent_index {
                                return;
                            }
                            state.last_sent_index = idx;
                            let pid = ParameterId::new(&param).unwrap();
                            gr.send(CommandEnum::SetParameter(SetParameter::new(
                                PortId::param(nid, 0),
                                pid,
                                table[index].clone(),
                                SignalOrigin::Automaton(serv_id.clone()),
                            )));
                            return;
                        }

                        let mapped = map.apply(raw);
                        let base = state.base;
                        let value = match ctrl {
                            ControlStrategy::Absolute => min + mapped * (max - min),
                            ControlStrategy::Modulation { depth } => {
                                (base + mapped * depth * (max - min)).clamp(min, max)
                            }
                        };
                        if (value - state.last_sent_value).abs() < 1e-6 {
                            return;
                        }
                        state.last_sent_value = value;

                        let pid = ParameterId::new(&param).unwrap();
                        gr.send(CommandEnum::SetParameter(SetParameter::new(
                            PortId::param(nid, 0),
                            pid,
                            ParamValue::Float(value as f32),
                            SignalOrigin::Automaton(serv_id.clone()),
                        )));
                    }
                    CommandEnum::Automaton(AutomatonCommand::SetEnabled { enabled, .. }) => {
                        s.lock().unwrap().enabled = enabled;
                    }
                    CommandEnum::Automaton(AutomatonCommand::Reset { .. }) => {
                        s.lock().unwrap().internal = a.reset();
                    }
                    CommandEnum::Automaton(AutomatonCommand::UiValue { value, .. }) => {
                        let mut state = s.lock().unwrap();
                        let pid = ParameterId::new(&param).unwrap();
                        let cmd = SetParameter::new(
                            PortId::param(nid, 0),
                            pid,
                            ParamValue::Float(value as f32),
                            SignalOrigin::Automaton(serv_id.clone()),
                        );
                        match confl {
                            ConflictStrategy::TouchOverride => {
                                state.base = value;
                                state.frozen = true;
                                gr.send(CommandEnum::SetParameter(cmd));
                            }
                            ConflictStrategy::BasePlusModulation => {
                                state.base = value;
                            }
                            ConflictStrategy::LastWriteWins => {
                                gr.send(CommandEnum::SetParameter(cmd));
                            }
                        }
                    }
                    CommandEnum::Automaton(AutomatonCommand::UiRelease { .. }) => {
                        let mut state = s.lock().unwrap();
                        if state.frozen {
                            state.frozen = false;
                        }
                    }
                    _ => {}
                })
            },
            1,
        )
    }

    pub fn with_table(mut self, table: Vec<ParamValue>) -> Self {
        self.table = Some(table);
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

// =============================================================================
// 9. Module trait — unified interface for sensors
// =============================================================================

pub type BoxedModule = Box<dyn Module>;

pub trait Module: Send {
    fn id(&self) -> &str;
    fn handle(&self) -> Option<ActorRef<CommandEnum>> {
        None
    }
    fn set_enabled(&mut self, _enabled: bool) {}
    fn stop(&mut self);
}

// =============================================================================
// 10. Helper constructors
// =============================================================================

pub fn midi_cc(
    controller: u8,
    channel: Option<u8>,
    target_node: NodeId,
    target_param: &str,
    min: f32,
    max: f32,
    transform: Transform,
) -> Mapping {
    Mapping::new(
        EventPattern::MidiControl {
            channel,
            controller,
        },
        Target {
            node_id: target_node,
            param_name: target_param.to_string(),
            min,
            max,
        },
        transform,
    )
}

pub fn osc_address(
    address: &str,
    target_node: NodeId,
    target_param: &str,
    min: f32,
    max: f32,
    transform: Transform,
) -> Mapping {
    Mapping::new(
        EventPattern::OscAddress(address.to_string()),
        Target {
            node_id: target_node,
            param_name: target_param.to_string(),
            min,
            max,
        },
        transform,
    )
}

// =============================================================================
// 11. Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_mapping() {
        let node = NodeId(1);
        let mapping = midi_cc(7, Some(1), node, "volume", 0.0, 1.0, Transform::Linear);
        let event = ControlEvent::MidiControl {
            channel: 1,
            controller: 7,
            value: 64,
            normalized: 0.5,
        };
        assert!(mapping.matches(&event));
        let cmd = mapping.apply(&event).unwrap();
        assert_eq!(cmd.port.node_id(), node);
        assert_eq!(cmd.parameter.as_ref(), "volume");
        assert!((cmd.value.as_f32().unwrap() - 0.5).abs() < 1e-6);
    }
}
