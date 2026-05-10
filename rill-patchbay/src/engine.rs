//! Control and automation subsystem.
//!
//! Provides event mapping (MIDI/OSC → parameters), automaton-based
//! modulation (LFO, envelopes), and a two-thread model with lock-free
//! queues for control → audio communication.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use rill_core::prelude::*;
use rill_core::queues::{MpscQueue, SetParameter, SignalOrigin};
use rill_core_actor::{ActorCell, ActorRef};

// crossbeam removed: // crossbeam removed (dead code)

pub use crate::automaton::{EnvelopeAutomaton, LfoAutomaton, LfoWaveform, Range};
use crate::sensor::Sensor;
use crate::strategy::{ConflictStrategy, ControlStrategy, UiCommand};

// =============================================================================
// 1. Event patterns
// =============================================================================

/// A pattern for matching controller events.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventPattern {
    /// Any button.
    AnyButton,
    /// A button with a specific ID.
    ButtonId(u32),

    /// Any knob.
    AnyKnob,
    /// A knob with a specific ID.
    KnobId(u32),

    /// Any fader.
    AnyFader,
    /// A fader with a specific ID.
    FaderId(u32),

    /// Any MIDI message.
    AnyMidi,
    /// MIDI Control Change.
    MidiControl {
        /// MIDI channel (None = any channel).
        channel: Option<u8>,
        /// Controller number.
        controller: u8,
    },
    /// MIDI Note.
    MidiNote {
        /// MIDI channel (None = any channel).
        channel: Option<u8>,
        /// Note number (None = any note).
        note: Option<u8>,
    },

    /// MIDI Clock tick.
    MidiClock,

    /// MIDI Transport message (Start / Stop / Continue).
    MidiTransport {
        /// Transport kind (None = any transport message).
        kind: Option<MidiTransportKind>,
    },

    /// Exact OSC address.
    OscAddress(String),

    /// OSC address pattern (substring match).
    OscPattern(String),
}

impl EventPattern {
    /// Check whether the given event matches this pattern.
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

/// A controller event from hardware or protocol input.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq)]
pub enum ControlEvent {
    /// Button press/release.
    Button {
        /// Button identifier.
        id: u32,
        /// Whether the button is pressed.
        pressed: bool,
    },

    /// Rotary knob / encoder.
    Knob {
        /// Knob identifier.
        id: u32,
        /// Raw value.
        value: f32,
        /// Normalised value (0.0–1.0).
        normalized: f32,
    },

    /// Linear fader.
    Fader {
        /// Fader identifier.
        id: u32,
        /// Raw value.
        value: f32,
        /// Normalised value (0.0–1.0).
        normalized: f32,
    },

    /// MIDI Control Change.
    MidiControl {
        /// MIDI channel (0–15).
        channel: u8,
        /// Controller number (0–127).
        controller: u8,
        /// Raw controller value (0–127).
        value: u8,
        /// Normalised value (0.0–1.0).
        normalized: f32,
    },

    /// MIDI Note.
    MidiNote {
        /// MIDI channel (0–15).
        channel: u8,
        /// Note number (0–127).
        note: u8,
        /// Velocity (0–127).
        velocity: u8,
        /// Whether the note is on (true) or off (false).
        on: bool,
    },

    /// OSC message.
    Osc {
        /// OSC address pattern (e.g. `/filter/cutoff`).
        address: String,
        /// Message arguments.
        args: Vec<f32>,
    },

    /// MIDI Clock tick (24 per quarter note).
    MidiClock,

    /// MIDI Transport message.
    MidiTransport {
        /// Transport command kind.
        kind: MidiTransportKind,
    },
}

/// MIDI transport commands.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MidiTransportKind {
    /// Start playback.
    Start,
    /// Stop playback.
    Stop,
    /// Continue playback.
    Continue,
}

impl ControlEvent {
    /// Return the normalised value (0.0–1.0) if applicable.
    pub fn normalized_value(&self) -> Option<f32> {
        match self {
            ControlEvent::Knob { normalized, .. } => Some(*normalized),
            ControlEvent::Fader { normalized, .. } => Some(*normalized),
            ControlEvent::MidiControl { normalized, .. } => Some(*normalized),
            ControlEvent::Button { pressed, .. } => Some(if *pressed { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    /// Return the controller element ID, if any.
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
// 2b. OSC Surface — OSC → EventPattern bridge
// =============================================================================

/// Maps an OSC address pattern to an internal [`EventPattern`].
///
/// One patchbay configuration can have a single canonical surface.
/// For alternate MIDI layouts, use separate `mappings` slices with
/// different `EventPattern::MidiControl` entries.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct OscSurfaceEntry {
    /// OSC address pattern, e.g. `"/delay/time"`.
    pub osc_path: String,

    /// Abstract controller identifier that `mappings` expect.
    pub event_pattern: EventPattern,

    /// Optional human-readable label (ignored by the engine).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub label: Option<String>,
}

/// A list of [`OscSurfaceEntry`] entries.
pub type OscSurface = Vec<OscSurfaceEntry>;

// =============================================================================
// 3. Value transforms
// =============================================================================

/// Type of value transformation.
#[derive(Clone)]
pub enum Transform {
    /// Linear: out = min + value * (max - min).
    Linear,

    /// Exponential: out = min + value² * (max - min).
    Exponential,

    /// Logarithmic: out = min + log₁₀(1 + value * 9) / log₁₀(10) * (max - min).
    Logarithmic,

    /// Inverted: out = max - value * (max - min).
    Inverted,

    /// Custom user-defined function.
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
    /// Apply the transform to a normalised value (0–1).
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

/// A target parameter on a graph node.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Target {
    /// Node ID in the signal graph.
    pub node_id: NodeId,
    /// Parameter name.
    pub param_name: String,
    /// Minimum value.
    pub min: f32,
    /// Maximum value.
    pub max: f32,
}

/// A mapping from an event pattern to a parameter target.
#[derive(Debug, Clone)]
pub struct Mapping {
    /// Event pattern to match.
    pub pattern: EventPattern,
    /// Target parameter.
    pub target: Target,
    /// Value transformation.
    pub transform: Transform,
    /// Human-readable name (for debugging).
    pub name: String,
    /// Whether this mapping is active.
    pub enabled: bool,
}

impl Mapping {
    /// Create a new mapping.
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

    /// Check whether an event matches this mapping's pattern.
    pub fn matches(&self, event: &ControlEvent) -> bool {
        self.enabled && self.pattern.matches(event)
    }

    /// Apply an event and produce a parameter command, if it matches.
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

/// Time type used by automata.
pub type Time = f64;

/// Marker for automata that need no external action.
#[derive(Debug, Clone, Default)]
pub struct NoAction;

/// Core trait for all automata.
///
/// An automaton is a generator: `(internal, current, time, action) → new_value`.
/// - `internal` — mutable automaton-specific state (phase, RNG, step counter, ...)
/// - `current` — current value at the control port (for reference, immutable)
/// - `time` — wall-clock seconds since start
/// - `action` — optional action to apply
///
/// `&self` is the immutable automaton configuration (frequency, waveform, ...).
pub trait Automaton: Send + Sync + Debug {
    /// Internal mutable state type (stored by Servo alongside the control port value).
    type Internal: Clone + Send + Sync + 'static;

    /// Action type (a pure function applied to the state).
    type Action: Debug + Clone + Send + Sync + Default + 'static;

    /// Compute the next control port value.
    fn step(
        &self,
        internal: &mut Self::Internal,
        current: &ParamValue,
        time: Time,
        action: &Self::Action,
    ) -> ParamValue;

    /// Initial internal state.
    fn initial_internal(&self) -> Self::Internal;

    /// Reset the automaton to its initial internal state.
    fn reset(&self) -> Self::Internal {
        self.initial_internal()
    }

    /// Automaton name.
    fn name(&self) -> &str;
}

// =============================================================================
// 6. AutomatonMsg + Servo — automaton-to-parameter bridge
// =============================================================================

/// Message for automaton actors — clock tick or control command.
#[derive(Debug, Clone)]
pub enum AutomatonMsg {
    /// Clock tick from audio thread.
    Tick(ClockTick),
    /// Enable or disable the automaton.
    SetEnabled(bool),
    /// Reset to initial state.
    Reset,
    /// UI command (from handle_event via patchbay).
    Ui(UiCommand),
}

/// Mapping type for a servo's output value.
#[derive(Clone)]
pub enum ParameterMapping {
    /// Linear: `min + value * (max - min)`.
    Linear,
    /// Exponential: `min + value^exp * (max - min)`.
    Exponential,
    /// Logarithmic: `min + log(1 + value * (e - 1)) / log(e) * (max - min)`.
    Logarithmic,
    /// Inverted linear: `max - value * (max - min)`.
    Inverted,
    /// Custom mapping function.
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
    /// Apply the mapping to a raw automaton value.
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

/// A servo bridges an automaton to a graph-node parameter.
///
/// Stores the automaton state externally (the automaton itself is a pure
/// function), provides an [`ActorRef`] for receiving control messages,
/// and sends output directly to the graph via `ActorRef<SetParameter>`.
pub struct Servo<A: Automaton> {
    id: String,
    automaton: A,
    internal: A::Internal,
    state: ParamValue,
    enabled: bool,
    mailbox: Arc<MpscQueue<AutomatonMsg>>,
    target_node: NodeId,
    target_param: String,
    mapping: ParameterMapping,
    min: f64,
    max: f64,
    last_sent_value: f64,
    table: Option<Vec<ParamValue>>,
    last_sent_index: i64,
    // ── Conflict resolution (ex-PortCombiner) ──
    control: ControlStrategy,
    conflict: ConflictStrategy,
    frozen: bool,
    base: f64,
}

impl<A: Automaton> Servo<A> {
    /// Create a servo.
    pub fn new(
        id: impl Into<String>,
        automaton: A,
        target_node: NodeId,
        target_param: impl Into<String>,
        /// Parameter mapping (kept for backward compat, superseded by ControlStrategy).
        #[allow(dead_code)]
        mapping: ParameterMapping,
        min: f64,
        max: f64,
    ) -> Self {
        let mut internal = automaton.initial_internal();
        let state = automaton.step(
            &mut internal,
            &ParamValue::Float(0.0),
            0.0,
            &A::Action::default(),
        );
        Self {
            id: id.into(),
            automaton,
            internal,
            state,
            enabled: true,
            mailbox: Arc::new(MpscQueue::with_capacity(16)),
            target_node,
            target_param: target_param.into(),
            mapping,
            min,
            max,
            last_sent_value: f64::NAN,
            table: None,
            last_sent_index: -1,
            control: ControlStrategy::Absolute,
            conflict: ConflictStrategy::LastWriteWins,
            frozen: false,
            base: (min + max) / 2.0,
        }
    }

    /// Create a servo driven by a sequence table.
    ///
    /// The automaton returns `ParamValue::Float(index)` and the servo looks up
    /// the actual `ParamValue` from the provided table.
    pub fn with_table(
        id: impl Into<String>,
        automaton: A,
        target_node: NodeId,
        target_param: impl Into<String>,
        table: Vec<ParamValue>,
    ) -> Self {
        let mut s = Self::new(
            id,
            automaton,
            target_node,
            target_param,
            ParameterMapping::Linear,
            0.0,
            1.0,
        );
        s.table = Some(table);
        s
    }

    /// Return an [`ActorRef`] for sending control messages.
    pub fn handle(&self) -> ActorRef<AutomatonMsg> {
        ActorRef::new(&self.mailbox)
    }

    /// Advance the servo and return a parameter command if the value changed.
    ///
    /// Drains the command queue before stepping the automaton.
    pub fn update(&mut self, time: Time) -> Option<SetParameter> {
        while let Some(cmd) = self.mailbox.pop() {
            match cmd {
                AutomatonMsg::SetEnabled(enabled) => self.enabled = enabled,
                AutomatonMsg::Reset => self.internal = self.automaton.reset(),
                AutomatonMsg::Tick(_) => {}
                AutomatonMsg::Ui(UiCommand::SetValue(v)) => match self.conflict {
                    ConflictStrategy::TouchOverride => {
                        self.base = v;
                        self.frozen = true;
                        return Some(self.make_cmd(v));
                    }
                    ConflictStrategy::BasePlusModulation => {
                        self.base = v;
                    }
                    ConflictStrategy::LastWriteWins => {
                        return Some(self.make_cmd(v));
                    }
                },
                AutomatonMsg::Ui(UiCommand::Release) => {
                    if self.frozen {
                        self.frozen = false;
                    }
                }
            }
        }

        if !self.enabled {
            return None;
        }

        if self.frozen && matches!(self.conflict, ConflictStrategy::TouchOverride) {
            return None;
        }

        let action = A::Action::default();
        self.state = self
            .automaton
            .step(&mut self.internal, &self.state, time, &action);

        let raw = self.state.as_f32().unwrap_or(0.0) as f64;

        if let Some(ref table) = self.table {
            let index = raw as usize;
            if index >= table.len() {
                return None;
            }
            let idx = index as i64;
            if idx == self.last_sent_index {
                return None;
            }
            self.last_sent_index = idx;
            return Some(self.make_cmd_from(table[index].clone()));
        }

        // F64 mode: apply ParameterMapping then ControlStrategy
        let mapped = self.mapping.apply(raw);
        let value = match self.control {
            ControlStrategy::Absolute => self.min + mapped * (self.max - self.min),
            ControlStrategy::Modulation { depth } => {
                (self.base + mapped * depth * (self.max - self.min)).clamp(self.min, self.max)
            }
        };

        if (value - self.last_sent_value).abs() < 1e-6 {
            return None;
        }
        self.last_sent_value = value;
        Some(self.make_cmd(value))
    }

    fn make_cmd(&self, value: f64) -> SetParameter {
        let pid = ParameterId::new(&self.target_param).unwrap();
        SetParameter::new(
            PortId::param(self.target_node, 0),
            pid,
            ParamValue::Float(value as f32),
            SignalOrigin::Automaton(self.id.clone()),
        )
    }

    fn make_cmd_from(&self, value: ParamValue) -> SetParameter {
        let pid = ParameterId::new(&self.target_param).unwrap();
        SetParameter::new(
            PortId::param(self.target_node, 0),
            pid,
            value,
            SignalOrigin::Automaton(self.id.clone()),
        )
    }

    /// Enable or disable this servo.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Return the servo's unique identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}

// ── Actor pattern: Servo is an actor ───────────────────────────────

impl<A: Automaton + 'static> ActorCell for Servo<A> {
    type Msg = AutomatonMsg;

    fn receive(&mut self, msg: AutomatonMsg) {
        match msg {
            AutomatonMsg::Tick(_) => {}
            AutomatonMsg::SetEnabled(enabled) => self.enabled = enabled,
            AutomatonMsg::Reset => self.internal = self.automaton.reset(),
            AutomatonMsg::Ui(_) => {} // handled in update()
        }
    }
}

/// Wrapper: converts a `Sensor` into a `Module` for unified storage.
struct SensorModule(Box<dyn Sensor>);

impl Module for SensorModule {
    fn id(&self) -> &str {
        "sensor"
    }
    fn stop(&mut self) {
        self.0.stop();
    }
}

/// Type-erased rack module (servo or sensor).
pub type BoxedModule = Box<dyn Module>;

/// Trait for rack modules — unified interface for servos and sensors.
pub trait Module: Send {
    /// Update the module and produce a parameter command (servos only).
    fn update(&mut self, _time: Time) -> Option<SetParameter> {
        None
    }
    /// Return the module's unique identifier.
    fn id(&self) -> &str;
    /// Return an ActorRef for sending control messages (servos only).
    fn handle(&self) -> Option<ActorRef<AutomatonMsg>> {
        None
    }
    /// Enable or disable the module (servos only, no-op for sensors).
    fn set_enabled(&mut self, _enabled: bool) {}
    /// Stop the module (disable, shutdown tasks, release resources).
    fn stop(&mut self);
}

impl<A: Automaton + 'static> Module for Servo<A> {
    fn update(&mut self, time: Time) -> Option<SetParameter> {
        Servo::update(self, time)
    }
    fn id(&self) -> &str {
        Servo::id(self)
    }
    fn handle(&self) -> Option<ActorRef<AutomatonMsg>> {
        Some(Servo::handle(self))
    }
    fn set_enabled(&mut self, enabled: bool) {
        self.set_enabled(enabled);
    }
    fn stop(&mut self) {
        self.set_enabled(false);
    }
}

// =============================================================================
// 8. Main patchbay controller
// =============================================================================

/// The central patchbay controller.
///
/// Operates on the **control thread** (soft RT) and sends parameter commands
/// to the audio thread via [`MpscQueue<SetParameter>`](rill_core::queues::MpscQueue).
///
/// ## Operation modes
///
/// - **Sync** (legacy): [`update(dt)`](Self::update) walks all servos sequentially.
///   Does not require tokio.
/// - **Async** (recommended): automata run as tokio tasks through
///   [`add_automaton_task()`](Self::add_automaton_task). Requires an active
///   tokio runtime.
pub struct Patchbay {
    mappings: Vec<Mapping>,
    modules: HashMap<String, BoxedModule>,
    automaton_handles: HashMap<String, tokio::task::JoinHandle<()>>,
    command_queue: ActorRef<SetParameter>,
    clock_mailbox: Arc<MpscQueue<ClockTick>>,
    event_mailbox: Arc<MpscQueue<ControlEvent>>,
    time: Time,
}

impl Patchbay {
    /// Create a new patchbay controller.
    ///
    /// Async methods (green threads, PortCombiner) require an active
    /// tokio runtime and will panic otherwise. Synchronous methods
    /// (servo, mapping, update) work without tokio.
    pub fn new(command_queue: ActorRef<SetParameter>) -> Self {
        Self {
            mappings: Vec::new(),
            modules: HashMap::new(),
            automaton_handles: HashMap::new(),
            command_queue,
            clock_mailbox: Arc::new(MpscQueue::with_capacity(16)),
            event_mailbox: Arc::new(MpscQueue::with_capacity(64)),
            time: 0.0,
        }
    }

    /// Return an [`ActorRef`] for the graph to send `ClockTick` to.
    pub fn clock_handle(&self) -> ActorRef<ClockTick> {
        ActorRef::new(&self.clock_mailbox)
    }

    /// Return an [`ActorRef`] for sensors (MIDI, OSC, knobs) to send events to.
    pub fn event_handle(&self) -> ActorRef<ControlEvent> {
        ActorRef::new(&self.event_mailbox)
    }

    /// Add an event mapping.
    pub fn add_mapping(&mut self, mapping: Mapping) {
        self.mappings.push(mapping);
    }

    /// Add a pre-constructed boxed servo.
    ///
    /// Useful for automaton types not covered by `add_lfo` / `add_envelope`
    /// (e.g. sequencers, named functions).
    pub fn add_boxed_servo(
        &mut self,
        id: String,
        servo: BoxedModule,
    ) -> Option<ActorRef<AutomatonMsg>> {
        let handle = servo.handle();
        self.modules.insert(id, servo);
        handle
    }

    /// Add a mapping from string descriptions (convenient for scripting).
    ///
    /// # Errors
    ///
    /// Returns `Err` if the pattern string is malformed.
    pub fn add_mapping_str(
        &mut self,
        pattern: &str,
        target_node: NodeId,
        target_param: &str,
        min: f32,
        max: f32,
        transform: Transform,
    ) -> Result<(), &'static str> {
        let pattern = match pattern {
            p if p.starts_with("button:") => {
                let id = p[7..].parse().map_err(|_| "Invalid button ID")?;
                EventPattern::ButtonId(id)
            }
            p if p.starts_with("knob:") => {
                let id = p[5..].parse().map_err(|_| "Invalid knob ID")?;
                EventPattern::KnobId(id)
            }
            p if p.starts_with("fader:") => {
                let id = p[6..].parse().map_err(|_| "Invalid fader ID")?;
                EventPattern::FaderId(id)
            }
            p if p.starts_with("midi:") => {
                let parts: Vec<&str> = p[5..].split(':').collect();
                if parts.len() == 2 {
                    let channel = parts[0].parse().ok();
                    let controller = parts[1].parse().map_err(|_| "Invalid controller")?;
                    EventPattern::MidiControl {
                        channel,
                        controller,
                    }
                } else {
                    EventPattern::AnyMidi
                }
            }
            p if p.starts_with("osc:") => EventPattern::OscAddress(p[4..].to_string()),
            _ => return Err("Unknown pattern"),
        };

        let target = Target {
            node_id: target_node,
            param_name: target_param.to_string(),
            min,
            max,
        };

        self.add_mapping(Mapping::new(pattern, target, transform));
        Ok(())
    }

    /// Add a servo (automaton → parameter bridge).
    /// Returns an [`ActorRef`] for sending control messages.
    pub fn add_servo<A: Automaton + 'static>(&mut self, servo: Servo<A>) -> ActorRef<AutomatonMsg> {
        let handle = servo.handle();
        self.modules.insert(servo.id().to_string(), Box::new(servo));
        handle
    }

    /// Add an LFO as a servo.
    pub fn add_lfo(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        target_node: NodeId,
        target_param: &str,
        min: f64,
        max: f64,
    ) {
        let automaton = LfoAutomaton::new(id, frequency, amplitude, offset, waveform);
        let servo = Servo::new(
            id,
            automaton,
            target_node,
            target_param,
            ParameterMapping::Linear,
            min,
            max,
        );
        self.add_servo(servo);
    }

    /// Add an envelope ADSR as a servo.
    pub fn add_envelope(
        &mut self,
        id: &str,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        target_node: NodeId,
        target_param: &str,
        min: f64,
        max: f64,
    ) {
        let automaton = EnvelopeAutomaton::adsr(id, attack, decay, sustain, release);
        let servo = Servo::new(
            id,
            automaton,
            target_node,
            target_param,
            ParameterMapping::Linear,
            min,
            max,
        );
        self.add_servo(servo);
    }

    /// Stop all modules — servos and sensors.
    pub fn stop_all(&mut self) {
        self.automaton_handles.clear();
        for module in self.modules.values_mut() {
            module.stop();
        }
        self.modules.clear();
    }

    /// Add a sensor (MIDI, OSC, etc.) to the rack.
    ///
    /// The sensor should already be started via `Sensor::start()`.
    /// Use `sensor.attach(self.event_handle())` before `start()`.
    pub fn add_sensor(&mut self, id: &str, sensor: Box<dyn Sensor>) {
        self.modules
            .insert(id.to_string(), Box::new(SensorModule(sensor)));
    }

    /// Process an incoming control event (MIDI, OSC, button, etc.).
    ///
    /// If a Servo exists for the target port, the event is routed via
    /// `AutomatonMsg::Ui` for conflict resolution; otherwise it is pushed
    /// directly to the command queue.
    pub fn handle_event(&mut self, event: ControlEvent) {
        for mapping in &self.mappings {
            if let Some(cmd) = mapping.apply(&event) {
                let key = target_key(cmd.port.node_id(), cmd.parameter.as_ref());
                if let Some(servo) = self.modules.get(&key) {
                    if let Some(ref servo_handle) = servo.handle() {
                        servo_handle.send(AutomatonMsg::Ui(UiCommand::SetValue(
                            cmd.value.as_f32().unwrap_or(0.0) as f64,
                        )));
                    }
                } else {
                    self.command_queue.send(cmd);
                }
            }
        }
    }

    /// Update synchronous servos.
    ///
    /// This method is deprecated. For new projects use `add_automaton_task()`
    /// with green threads.
    pub fn update(&mut self, dt: f32) {
        self.time += dt as f64;

        for servo in self.modules.values_mut() {
            if let Some(cmd) = servo.update(self.time) {
                self.command_queue.send(cmd);
            }
        }
    }

    /// Return a clone of the command queue ActorRef for async task spawning.
    pub fn command_queue(&self) -> ActorRef<SetParameter> {
        self.command_queue.clone()
    }

    /// Store a task handle for lifecycle management (async automaton tasks).
    pub fn store_task_handle(&mut self, id: String, handle: tokio::task::JoinHandle<()>) {
        self.automaton_handles.insert(id, handle);
    }

    /// Return all mappings.
    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }

    /// Get a servo by ID.
    pub fn get_servo(&self, id: &str) -> Option<&dyn Module> {
        self.modules.get(id).map(|b| b.as_ref())
    }

    /// Get a mutable servo by ID.
    pub fn get_servo_mut(&mut self, id: &str) -> Option<&mut BoxedModule> {
        self.modules.get_mut(id)
    }

    /// Remove a servo by ID.
    pub fn remove_servo(&mut self, id: &str) -> bool {
        self.modules.remove(id).is_some()
    }

    /// Clear all mappings, servos, and async automata.
    pub fn clear(&mut self) {
        self.mappings.clear();
        self.modules.clear();
        self.stop_all();
    }

    /// Reset the internal clock to zero.
    pub fn reset_time(&mut self) {
        self.time = 0.0;
    }

    /// Current internal time in seconds.
    pub fn current_time(&self) -> Time {
        self.time
    }

    /// Drain the clock mailbox and broadcast to all servos.
    ///
    /// Call this from the main loop (not the audio thread).
    pub fn drain_clock(&mut self) {
        self.drain_events();
        while let Some(clock) = self.clock_mailbox.pop() {
            let msg = AutomatonMsg::Tick(clock);
            for servo in self.modules.values() {
                if let Some(ref handle) = servo.handle() {
                    handle.send(msg.clone());
                }
            }
            let dt = clock.samples_since_last as f64 / clock.sample_rate as f64;
            self.time += dt;
            self.update(dt as f32);
        }
    }

    /// Drain the event mailbox and dispatch to mappings.
    pub fn drain_events(&mut self) {
        while let Some(event) = self.event_mailbox.pop() {
            self.handle_event(event);
        }
    }

    /// Spawn a periodic tokio task that drains the clock mailbox.
    ///
    /// Returns a `JoinHandle` for lifecycle management.
    pub fn spawn_clock_loop(
        patchbay: Arc<std::sync::Mutex<Patchbay>>,
        interval: std::time::Duration,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                if let Ok(mut pb) = patchbay.lock() {
                    pb.drain_clock();
                }
            }
        })
    }
}

// =============================================================================
// 9. Helper constructors
// =============================================================================
// 9. Helper functions for creating mappings
// =============================================================================

/// Create a MIDI CC → parameter mapping.
pub fn midi_cc(
    controller: u8,
    channel: Option<u8>,
    target_node: NodeId,
    target_param: &str,
    min: f32,
    max: f32,
    transform: Transform,
) -> Mapping {
    let pattern = EventPattern::MidiControl {
        channel,
        controller,
    };
    let target = Target {
        node_id: target_node,
        param_name: target_param.to_string(),
        min,
        max,
    };
    Mapping::new(pattern, target, transform)
}

/// Create an OSC address → parameter mapping.
pub fn osc_address(
    address: &str,
    target_node: NodeId,
    target_param: &str,
    min: f32,
    max: f32,
    transform: Transform,
) -> Mapping {
    let pattern = EventPattern::OscAddress(address.to_string());
    let target = Target {
        node_id: target_node,
        param_name: target_param.to_string(),
        min,
        max,
    };
    Mapping::new(pattern, target, transform)
}

// =============================================================================
// 9b. PortCombiner key helper
// =============================================================================

fn target_key(node_id: NodeId, param_name: &str) -> String {
    format!("{}:{}", node_id.inner(), param_name)
}

// =============================================================================
// 10. Tests
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

    #[test]
    fn test_lfo_servo() {
        let node = NodeId(1);
        let (actor_ref, _mailbox) = ActorRef::new_pair();
        let mut control = Patchbay::new(actor_ref);

        control.add_lfo(
            "test_lfo",
            1.0,
            0.5,
            0.0,
            LfoWaveform::Sine,
            node,
            "cutoff",
            100.0,
            1000.0,
        );

        assert!(control.get_servo("test_lfo").is_some());

        for _i in 0..10 {
            control.update(0.1);
        }
    }

    #[test]
    fn test_envelope_servo() {
        let node = NodeId(1);
        let (actor_ref, _mailbox) = ActorRef::new_pair();
        let mut control = Patchbay::new(actor_ref);

        control.add_envelope("test_env", 0.1, 0.2, 0.7, 0.3, node, "gain", 0.0, 1.0);

        if let Some(_servo) = control.get_servo_mut("test_env") {}

        control.update(0.05);
        control.update(0.05);
    }
}
