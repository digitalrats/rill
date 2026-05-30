//! Control and automation subsystem.
//!
//! Provides event mapping (MIDI/OSC → parameters), automaton-based
//! modulation (LFO, envelopes), and a two-thread model with lock-free
//! queues for control → signal communication.

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

/// What aspect of a MIDI note event to extract for mapping.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MidiNoteKind {
    /// Extracts frequency: `midi_to_freq(note)`. Note Off produces no value.
    Frequency,
    /// Extracts amplitude: `velocity / 127` (On) or `0.0` (Off).
    #[default]
    Amplitude,
    /// Extracts gate: `1.0` (On) or `0.0` (Off).
    Gate,
}

/// A pattern for matching controller events.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventPattern {
    /// Matches any button event regardless of ID.
    AnyButton,
    /// Matches a button event with a specific hardware ID.
    ButtonId(u32),
    /// Matches any knob event regardless of ID.
    AnyKnob,
    /// Matches a knob event with a specific hardware ID.
    KnobId(u32),
    /// Matches any fader event regardless of ID.
    AnyFader,
    /// Matches a fader event with a specific hardware ID.
    FaderId(u32),
    /// Matches any MIDI event (control change, note, clock, or transport).
    AnyMidi,
    /// Matches a MIDI control change event by controller number and optional channel.
    MidiControl {
        /// Optional MIDI channel filter; `None` matches any channel.
        channel: Option<u8>,
        /// MIDI controller number (CC index).
        controller: u8,
    },
    /// Matches a MIDI note-on or note-off event and extracts a mapped value.
    MidiNote {
        /// Optional MIDI channel filter; `None` matches any channel.
        channel: Option<u8>,
        /// Optional note number filter; `None` matches any note.
        note: Option<u8>,
        /// Which aspect of the note event to use as the mapping value.
        #[cfg_attr(feature = "serde", serde(default))]
        kind: MidiNoteKind,
    },
    /// Matches a MIDI clock tick event.
    MidiClock,
    /// Matches a MIDI transport event (start, stop, or continue).
    MidiTransport {
        /// Optional transport kind filter; `None` matches any transport event.
        kind: Option<MidiTransportKind>,
    },
    /// Matches an OSC message by exact address string.
    OscAddress(String),
    /// Matches an OSC message whose address contains the given substring.
    OscPattern(String),
}

impl EventPattern {
    /// Checks whether this pattern matches a given control event.
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
                EventPattern::MidiNote { channel, note, .. },
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
/// Hardware control event from a physical interface (knob, button, fader, etc.).
pub enum ControlEvent {
    /// A physical button press or release.
    Button {
        /// Hardware control identifier.
        id: u32,
        /// `true` if the button is currently held down.
        pressed: bool,
    },
    /// A physical knob (rotary encoder or potentiometer) event.
    Knob {
        /// Hardware control identifier.
        id: u32,
        /// Raw value in hardware-native units.
        value: f32,
        /// Value mapped to the [0.0, 1.0] range.
        normalized: f32,
    },
    /// A physical fader (linear slider) event.
    Fader {
        /// Hardware control identifier.
        id: u32,
        /// Raw value in hardware-native units.
        value: f32,
        /// Value mapped to the [0.0, 1.0] range.
        normalized: f32,
    },
    /// A MIDI control change message.
    MidiControl {
        /// MIDI channel (0-indexed).
        channel: u8,
        /// MIDI controller number.
        controller: u8,
        /// Raw 7-bit MIDI value.
        value: u8,
        /// Value normalized to [0.0, 1.0].
        normalized: f32,
    },
    /// A MIDI note-on or note-off message.
    MidiNote {
        /// MIDI channel (0-indexed).
        channel: u8,
        /// MIDI note number.
        note: u8,
        /// MIDI velocity value (0-127).
        velocity: u8,
        /// `true` for note-on, `false` for note-off.
        on: bool,
    },
    /// An OSC message event.
    Osc {
        /// OSC address path.
        address: String,
        /// OSC argument list as float values.
        args: Vec<f32>,
    },
    /// A MIDI clock tick event.
    MidiClock,
    /// A MIDI transport state change.
    MidiTransport {
        /// The type of transport event (start, stop, or continue).
        kind: MidiTransportKind,
    },
}

/// MIDI transport state.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MidiTransportKind {
    /// Transport started.
    Start,
    /// Transport stopped.
    Stop,
    /// Transport resumed from current position.
    Continue,
}

impl ControlEvent {
    /// Returns the normalized value (0.0–1.0) of this event, if it carries one.
    pub fn normalized_value(&self) -> Option<f32> {
        match self {
            ControlEvent::Knob { normalized, .. } => Some(*normalized),
            ControlEvent::Fader { normalized, .. } => Some(*normalized),
            ControlEvent::MidiControl { normalized, .. } => Some(*normalized),
            ControlEvent::Button { pressed, .. } => Some(if *pressed { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    /// Returns the hardware control ID attached to this event, if any.
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

/// A single entry in an OSC control surface, binding an OSC path to an event pattern.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct OscSurfaceEntry {
    /// The OSC address path this entry listens to.
    pub osc_path: String,
    /// The event pattern that triggered actions should match.
    pub event_pattern: EventPattern,
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    /// Optional human-readable label for UI display.
    pub label: Option<String>,
}

/// A list of OSC address → event mappings forming a control surface layout.
pub type OscSurface = Vec<OscSurfaceEntry>;

// =============================================================================
// 3. Value transforms
// =============================================================================

/// Transfer function applied to a normalized [0,1] value before scaling to parameter range.
#[derive(Clone)]
pub enum Transform {
    /// Identity: value passes through unchanged.
    Linear,
    /// Square mapping: finer control near zero, coarser near one.
    Exponential,
    /// Logarithmic mapping: finer control near maximum.
    Logarithmic,
    /// Reversed mapping: 1.0 becomes min, 0.0 becomes max.
    Inverted,
    /// User-defined custom transfer function.
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
    /// Applies the transform to a normalized value, mapping it into the [min, max] range.
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

/// The destination of an event mapping: a specific parameter on a specific graph node.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Target {
    /// Graph node that owns the target parameter.
    pub node_id: NodeId,
    /// Name of the parameter to control.
    pub param_name: String,
    /// Lower bound of the parameter value range.
    pub min: f32,
    /// Upper bound of the parameter value range.
    pub max: f32,
}

/// A complete mapping from an input event to a target parameter, with a value transform.
#[derive(Debug, Clone)]
pub struct Mapping {
    /// Event pattern that triggers this mapping.
    pub pattern: EventPattern,
    /// Target parameter to set when the pattern matches.
    pub target: Target,
    /// Transform applied to the normalized event value before scaling.
    pub transform: Transform,
    /// Human-readable name for debugging and UI.
    pub name: String,
    /// Whether this mapping is currently active.
    pub enabled: bool,
}

impl Mapping {
    /// Creates a new mapping with an auto-generated name.
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

    /// Returns `true` if this mapping is enabled and matches the given event.
    pub fn matches(&self, event: &ControlEvent) -> bool {
        self.enabled && self.pattern.matches(event)
    }

    /// Produces a parameter-set command if the event matches this mapping.
    pub fn apply(&self, event: &ControlEvent) -> Option<SetParameter> {
        if !self.matches(event) {
            return None;
        }

        // MidiNote with kind: extract value from note event, bypassing
        // the standard normalized_value() pipeline.
        if let (
            EventPattern::MidiNote { kind, .. },
            ControlEvent::MidiNote {
                note, velocity, on, ..
            },
        ) = (&self.pattern, event)
        {
            let value = match kind {
                MidiNoteKind::Frequency => {
                    if !*on {
                        return None;
                    }
                    // midi_to_freq produces absolute Hz — bypass Transform
                    rill_core_dsp::math::midi_to_freq::<f32>(*note)
                }
                MidiNoteKind::Amplitude => {
                    let raw = if *on { *velocity as f32 / 127.0 } else { 0.0 };
                    self.transform.apply(raw, self.target.min, self.target.max)
                }
                MidiNoteKind::Gate => {
                    let raw = if *on { 1.0 } else { 0.0 };
                    self.transform.apply(raw, self.target.min, self.target.max)
                }
            };
            let pid = ParameterId::new(&self.target.param_name).unwrap();
            return Some(SetParameter::new(
                PortId::param(self.target.node_id, 0),
                pid,
                ParamValue::Float(value),
                SignalOrigin::External(self.name.clone()),
            ));
        }

        // All other patterns: use the standard normalized_value() pipeline.
        let norm = event.normalized_value()?;
        let value = self.transform.apply(norm, self.target.min, self.target.max);
        let pid = ParameterId::new(&self.target.param_name).unwrap();
        Some(SetParameter::new(
            PortId::param(self.target.node_id, 0),
            pid,
            ParamValue::Float(value),
            SignalOrigin::External(self.name.clone()),
        ))
    }
}

// =============================================================================
// 5. Automaton core trait
// =============================================================================

/// Time in seconds, used for automaton clocks and timekeeping.
pub type Time = f64;

/// A unit action for automatons that need no external action per step.
#[derive(Debug, Clone, Default)]
pub struct NoAction;

/// Core trait for automatons — stateful signal generators that advance per step.
pub trait Automaton: Send + Sync + Debug {
    /// The automaton's internal state, carried across step invocations.
    type Internal: Clone + Send + Sync + 'static;
    /// An optional action type driving state transitions on each step.
    type Action: Debug + Clone + Send + Sync + Default + 'static;

    /// Advances the automaton by one step, producing a new output value.
    ///
    /// `internal` holds mutable state, `current` is the last output value,
    /// `time` is the elapsed time in seconds, and `action` is an optional trigger.
    fn step(
        &self,
        internal: &mut Self::Internal,
        current: &ParamValue,
        time: Time,
        action: &Self::Action,
    ) -> ParamValue;

    /// Returns the automaton's initial internal state (at time zero).
    fn initial_internal(&self) -> Self::Internal;

    /// Resets the automaton to its initial internal state.
    fn reset(&self) -> Self::Internal {
        self.initial_internal()
    }

    /// Returns the human-readable name of this automaton.
    fn name(&self) -> &str;
}

// =============================================================================
// 6. Parameter mapping
// =============================================================================

/// Transfer function for mapping raw automaton output [0,1] to parameter space.
#[derive(Clone)]
pub enum ParameterMapping {
    /// Identity: output equals input.
    Linear,
    /// Square mapping: finer control near zero.
    Exponential,
    /// Logarithmic mapping: finer control near maximum.
    Logarithmic,
    /// Inverted: 1.0 maps to 0.0 and vice versa.
    Inverted,
    /// User-defined custom mapping function.
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
    /// Applies this mapping to a raw value in the [0, 1] range.
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

/// Internal runtime state of a Servo, shared between the control actor and automation logic.
pub(crate) struct ServoState<A: Automaton> {
    /// Current automaton internal state.
    pub(crate) internal: A::Internal,
    /// Most recent output value produced by the automaton.
    pub(crate) value: ParamValue,
    /// Elapsed time in seconds since the automaton started.
    pub(crate) time: Time,
    /// Whether the servo is actively stepping the automaton.
    pub(crate) enabled: bool,
    /// Base value for modulation strategies (offset added to modulation output).
    pub(crate) base: f64,
    /// When `true`, the servo is frozen from UI touch (used with TouchOverride).
    pub(crate) frozen: bool,
    /// Last value sent to the graph, used for change detection.
    pub(crate) last_sent_value: f64,
    /// Last table index sent (only used with value tables).
    pub(crate) last_sent_index: i64,
}

// =============================================================================
// 8. Servo — automaton-to-parameter bridge
// =============================================================================

/// Bridges an automaton to a graph parameter, stepping on every clock tick and
/// sending control commands to the signal graph.
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
    /// Creates a new Servo linking an automaton to a target parameter.
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

    /// Spawns this servo as a detached tokio actor, returning its address.
    ///
    /// The actor listens for `ClockTick` to step the automaton, and for
    /// `AutomatonCommand` variants to handle enable/reset/UI value events.
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

    /// Attaches a preset value table; raw automaton output selects table entries by index.
    pub fn with_table(mut self, table: Vec<ParamValue>) -> Self {
        self.table = Some(table);
        self
    }

    /// Returns this servo's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

// =============================================================================
// 9. Module trait — unified interface for sensors
// =============================================================================

/// Type-erased, heap-allocated reference to any module.
pub type BoxedModule = Box<dyn Module>;

/// Unified interface for sensor and control modules (MIDI hubs, OSC servers, etc.).
pub trait Module: Send {
    /// Returns this module's unique identifier.
    fn id(&self) -> &str;
    /// Returns the actor handle if this module has a control actor, `None` otherwise.
    fn handle(&self) -> Option<ActorRef<CommandEnum>> {
        None
    }
    /// Enables or disables the module.
    fn set_enabled(&mut self, _enabled: bool) {}
    /// Stops the module, joining any background threads.
    fn stop(&mut self);
}

// =============================================================================
// 10. Helper constructors
// =============================================================================

/// Convenience constructor for a MIDI control change mapping.
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

/// Convenience constructor for a MIDI note mapping.
///
/// Use [`MidiNoteKind`] to select which aspect of the note event to extract:
/// - `Frequency` — `midi_to_freq(note)`, Note Off produces no value
/// - `Amplitude` — `velocity / 127` (On) or `0.0` (Off)
/// - `Gate` — `1.0` (On) or `0.0` (Off)
pub fn midi_note(
    kind: MidiNoteKind,
    note: Option<u8>,
    channel: Option<u8>,
    target_node: NodeId,
    target_param: &str,
    min: f32,
    max: f32,
    transform: Transform,
) -> Mapping {
    Mapping::new(
        EventPattern::MidiNote {
            channel,
            note,
            kind,
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

/// Convenience constructor for an OSC address mapping.
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
