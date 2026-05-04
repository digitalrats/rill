//! Control and automation subsystem.
//!
//! Provides event mapping (MIDI/OSC → parameters), automaton-based
//! modulation (LFO, envelopes), and a two-thread model with lock-free
//! queues for control → audio communication.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use rill_core::prelude::*;
use rill_core::queues::telemetry::{Telemetry, CLOCK_TICK};
use rill_core::queues::MpscQueue;

use crossbeam_channel::Receiver as CrossbeamReceiver;

pub use crate::automaton::Range;
use crate::automaton::{EnvelopeAutomaton, LfoAutomaton, LfoWaveform};
use crate::automaton_task::spawn_automaton_task;
use crate::port_combiner::{spawn_combiner, PortCombinerHandle};
use crate::sequencer::{SequencerCommand, SequencerHandle, SnapshotSequencer};
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
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "Option::is_none"))]
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
    pub fn apply(&self, event: &ControlEvent) -> Option<ParameterCommand> {
        if !self.matches(event) {
            return None;
        }

        event.normalized_value().map(|norm| {
            let value = self.transform.apply(norm, self.target.min, self.target.max);
            ParameterCommand {
                node_id: self.target.node_id,
                param: self.target.param_name.clone(),
                value,
            }
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
/// An automaton is a stateful function generator. Each call to [`step`](Self::step)
/// takes the current time, an action, and the current state, and returns a
/// new state together with an optional output value.  Automata are `Send`
/// and run on the control thread (soft RT).
pub trait Automaton: Send + Sync + Debug {
    /// State type.
    type State: Clone + Send + Sync + 'static + Debug;

    /// Action type (a pure function applied to the state).
    type Action: Debug + Clone + Send + Sync + Default + 'static;

    /// Advance the automaton by one time step.
    ///
    /// # Arguments
    /// * `time` — current time
    /// * `action` — action to apply
    /// * `state` — current state
    ///
    /// Returns `(new_state, optional_output_value)`.
    fn step(
        &self,
        time: Time,
        action: &Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>);

    /// Return the initial state.
    fn initial_state(&self) -> Self::State;

    /// Automaton name.
    fn name(&self) -> &str;

    /// Extract the output value from the state.
    fn extract_value(&self, state: &Self::State) -> f64;

    /// Reset the automaton to its initial state.
    fn reset(&self) -> Self::State {
        self.initial_state()
    }
}

// =============================================================================
// 6. Servo — automaton-to-parameter bridge
// =============================================================================

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
pub struct Servo<A: Automaton> {
    id: String,
    automaton: A,
    state: A::State,
    target_node: NodeId,
    target_param: String,
    mapping: ParameterMapping,
    min: f64,
    max: f64,
    last_value: f64,
    enabled: bool,
    last_time: Time,
}

impl<A: Automaton> Servo<A> {
    /// Create a new servo.
    pub fn new(
        id: impl Into<String>,
        automaton: A,
        target_node: NodeId,
        target_param: impl Into<String>,
        mapping: ParameterMapping,
        min: f64,
        max: f64,
    ) -> Self {
        let state = automaton.initial_state();
        Self {
            id: id.into(),
            automaton,
            state,
            target_node,
            target_param: target_param.into(),
            mapping,
            min,
            max,
            last_value: 0.0,
            enabled: true,
            last_time: 0.0,
        }
    }

    /// Advance the servo and return a parameter command if the value changed.
    pub fn update(&mut self, time: Time) -> Option<ParameterCommand> {
        if !self.enabled {
            return None;
        }

        let (new_state, value_opt) = self
            .automaton
            .step(time, &A::Action::default(), &self.state);
        self.state = new_state;

        if let Some(raw_value) = value_opt {
            let mapped = self.mapping.apply(raw_value);
            let clamped = mapped.clamp(self.min, self.max);

            if (clamped - self.last_value).abs() > 1e-6 {
                self.last_value = clamped;
                self.last_time = time;

                return Some(ParameterCommand {
                    node_id: self.target_node,
                    param: self.target_param.clone(),
                    value: clamped as f32,
                });
            }
        }

        None
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

/// Type-erased boxed servo.
pub type BoxedServo = Box<dyn AnyServo>;

/// Trait for type-erased servo operations.
pub trait AnyServo: Send + Sync {
    /// Update the servo and return a parameter command if the value changed.
    fn update(&mut self, time: Time) -> Option<ParameterCommand>;
    /// Return the servo's unique identifier.
    fn id(&self) -> &str;
    /// Enable or disable the servo.
    fn set_enabled(&mut self, enabled: bool);
}

impl<A: Automaton + 'static> AnyServo for Servo<A> {
    fn update(&mut self, time: Time) -> Option<ParameterCommand> {
        Servo::update(self, time)
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

// =============================================================================
// 7. Parameter commands
// =============================================================================

/// A command to change a graph-node parameter, sent to the audio thread.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct ParameterCommand {
    /// Target node ID.
    pub node_id: NodeId,
    /// Parameter name.
    pub param: String,
    /// New value.
    pub value: f32,
}

impl ParameterCommand {
    /// Create a new parameter command.
    pub fn new(node_id: NodeId, param: impl Into<String>, value: f32) -> Self {
        Self {
            node_id,
            param: param.into(),
            value,
        }
    }
}

// =============================================================================
// 8. Main patchbay controller
// =============================================================================

/// The central patchbay controller.
///
/// Operates on the **control thread** (soft RT) and sends parameter commands
/// to the audio thread via [`MpscQueue<ParameterCommand>`].
///
/// ## Operation modes
///
/// - **Sync** (legacy): [`update(dt)`](Self::update) walks all servos sequentially.
///   Does not require tokio.
/// - **Async** (recommended): automata run as tokio tasks through
///   [`add_automaton_task()`](Self::add_automaton_task). Requires an active
///   tokio runtime.
pub struct PatchbayControl {
    mappings: Vec<Mapping>,
    servos: HashMap<String, BoxedServo>,
    port_combiners: HashMap<String, PortCombinerHandle>,
    automaton_handles: HashMap<String, tokio::task::JoinHandle<()>>,
    sequencer_handle: Option<SequencerHandle>,
    sequencer_task: Option<tokio::task::JoinHandle<()>>,
    command_queue: Arc<MpscQueue<ParameterCommand>>,
    time: Time,
}

impl PatchbayControl {
    /// Create a new patchbay controller.
    pub fn new(command_queue: Arc<MpscQueue<ParameterCommand>>) -> Self {
        Self {
            mappings: Vec::new(),
            servos: HashMap::new(),
            port_combiners: HashMap::new(),
            automaton_handles: HashMap::new(),
            sequencer_handle: None,
            sequencer_task: None,
            command_queue,
            time: 0.0,
        }
    }

    /// Add an event mapping.
    pub fn add_mapping(&mut self, mapping: Mapping) {
        self.mappings.push(mapping);
    }

    /// Add a pre-constructed boxed servo.
    ///
    /// Useful for automaton types not covered by `add_lfo` / `add_envelope`
    /// (e.g. sequencers, named functions).
    pub fn add_boxed_servo(&mut self, id: String, servo: BoxedServo) {
        self.servos.insert(id, servo);
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
    pub fn add_servo<A: Automaton + 'static>(&mut self, servo: Servo<A>) {
        self.servos.insert(servo.id().to_string(), Box::new(servo));
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

    /// Add an automaton as a green thread (tokio task).
    ///
    /// Requires an active tokio runtime. Ports with async automata receive
    /// a `PortCombiner` that resolves UI ↔ automaton conflicts.
    ///
    /// # Arguments
    ///
    /// * `id` — unique identifier
    /// * `automaton` — the automaton implementation
    /// * `interval` — update interval (e.g. 10 ms = 100 Hz)
    /// * `target` — `(node_id, param_name)`
    /// * `range` — `(min, max)` parameter range
    /// * `control` — control strategy
    /// * `conflict` — conflict resolution strategy
    pub fn add_automaton_task<A: Automaton + 'static>(
        &mut self,
        id: &str,
        automaton: A,
        interval: Duration,
        target: (NodeId, String),
        range: (f64, f64),
        control: ControlStrategy,
        conflict: ConflictStrategy,
    ) {
        let key = target_key(target.0, &target.1);

        let combiner = spawn_combiner(
            target,
            range,
            control,
            conflict,
            self.command_queue.clone(),
        );

        let task = spawn_automaton_task(
            automaton,
            interval,
            combiner.automaton_tx.clone(),
            combiner.cancel_rx(),
        );

        self.port_combiners.insert(key, combiner);
        self.automaton_handles.insert(id.to_string(), task);
    }

    /// Add an LFO as an async automaton task.
    pub fn add_lfo_task(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        interval: Duration,
        target: (NodeId, String),
        range: (f64, f64),
        control: ControlStrategy,
        conflict: ConflictStrategy,
    ) {
        let automaton = LfoAutomaton::new(id, frequency, amplitude, offset, waveform);
        self.add_automaton_task(
            format!("{}_auto", id).as_str(),
            automaton,
            interval,
            target,
            range,
            control,
            conflict,
        );
    }

    /// Add an envelope ADSR as an async automaton task.
    pub fn add_envelope_task(
        &mut self,
        id: &str,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        interval: Duration,
        target: (NodeId, String),
        range: (f64, f64),
        control: ControlStrategy,
        conflict: ConflictStrategy,
    ) {
        let automaton = EnvelopeAutomaton::adsr(id, attack, decay, sustain, release);
        self.add_automaton_task(
            format!("{}_auto", id).as_str(),
            automaton,
            interval,
            target,
            range,
            control,
            conflict,
        );
    }

    /// Attach a parameter-lock sequencer driven by audio-thread clock ticks.
    ///
    /// Spawns a blocking tokio task that reads `CLOCK_TICK` telemetry and
    /// pushes returned parameter commands to the queue.
    ///
    /// Returns a [`SequencerHandle`] for external control.
    ///
    /// # Panics
    ///
    /// Panics if a sequencer is already attached (call `detach_sequencer()` first).
    pub fn attach_sequencer(
        &mut self,
        tel_rx: CrossbeamReceiver<Telemetry>,
        sequencer: SnapshotSequencer,
    ) -> SequencerHandle {
        assert!(
            self.sequencer_task.is_none(),
            "sequencer already attached — detach first"
        );

        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<SequencerCommand>();
        let queue = self.command_queue.clone();

        let task = tokio::task::spawn_blocking(move || {
            let mut seq = sequencer;

            loop {
                loop {
                    match cmd_rx.try_recv() {
                        Ok(SequencerCommand::Start) => seq.start(),
                        Ok(SequencerCommand::Stop) => seq.stop(),
                        Ok(SequencerCommand::Reset { sample_pos }) => seq.reset(sample_pos),
                        Ok(SequencerCommand::SetPattern(id)) => seq.set_active_pattern(&id),
                        Err(crossbeam_channel::TryRecvError::Empty) => break,
                        Err(crossbeam_channel::TryRecvError::Disconnected) => return,
                    }
                }

                match tel_rx.recv() {
                    Ok(Telemetry::Event { kind, data, .. }) if kind == CLOCK_TICK => {
                        if data.len() >= 3 {
                            let sample_pos = data[0] as u64;
                            let sample_rate = data[1];
                            let tempo = data[2];

                            let beat_pos = data.get(3).copied().unwrap_or(0.0);
                            let new_beat = data.get(4).copied().unwrap_or(0.0) > 0.5;
                            let new_bar = data.get(5).copied().unwrap_or(0.0) > 0.5;

                            let cmds = seq.tick_ext(
                                sample_pos, sample_rate, tempo,
                                beat_pos, new_beat, new_bar,
                            );
                            for cmd in cmds {
                                let _ = queue.push(cmd);
                            }
                        }
                    }
                    Err(_) => return,
                    _ => {}
                }
            }
        });

        let handle = SequencerHandle::new(cmd_tx);
        self.sequencer_handle = Some(handle.clone());
        self.sequencer_task = Some(task);

        handle
    }

    /// Detach the sequencer: abort its task and drop the handle.
    pub fn detach_sequencer(&mut self) {
        if let Some(task) = self.sequencer_task.take() {
            task.abort();
        }
        self.sequencer_handle = None;
    }

    /// Get a reference to the sequencer handle, if attached.
    pub fn sequencer_handle(&self) -> Option<&SequencerHandle> {
        self.sequencer_handle.as_ref()
    }

    /// Stop all async automata and the sequencer.
    pub fn stop_all(&mut self) {
        for combiner in self.port_combiners.values() {
            combiner.stop();
        }
        self.port_combiners.clear();
        self.automaton_handles.clear();
        self.detach_sequencer();
    }

    /// Handle an external event (MIDI/OSC).
    ///
    /// If a `PortCombiner` exists for the target port the event is routed
    /// there for conflict resolution; otherwise it is pushed directly to
    /// the command queue.
    pub fn handle_event(&mut self, event: ControlEvent) {
        for mapping in &self.mappings {
            if let Some(cmd) = mapping.apply(&event) {
                let key = target_key(cmd.node_id, &cmd.param);
                if let Some(combiner) = self.port_combiners.get(&key) {
                    let _ = combiner.ui_tx.send(UiCommand::SetValue(cmd.value as f64));
                } else {
                    let _ = self.command_queue.push(cmd);
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

        for servo in self.servos.values_mut() {
            if let Some(cmd) = servo.update(self.time) {
                let _ = self.command_queue.push(cmd);
            }
        }
    }

    /// Get a combiner by key (format: `"node_id:param_name"`).
    pub fn get_combiner(&self, key: &str) -> Option<&PortCombinerHandle> {
        self.port_combiners.get(key)
    }

    /// Return all mappings.
    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }

    /// Get a servo by ID.
    pub fn get_servo(&self, id: &str) -> Option<&dyn AnyServo> {
        self.servos.get(id).map(|b| b.as_ref())
    }

    /// Get a mutable servo by ID.
    pub fn get_servo_mut(&mut self, id: &str) -> Option<&mut BoxedServo> {
        self.servos.get_mut(id)
    }

    /// Remove a servo by ID.
    pub fn remove_servo(&mut self, id: &str) -> bool {
        self.servos.remove(id).is_some()
    }

    /// Clear all mappings, servos, and async automata.
    pub fn clear(&mut self) {
        self.mappings.clear();
        self.servos.clear();
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
}

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
    format!("{}:{}", node_id.0, param_name)
}

// =============================================================================
// 10. Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rill_core::queues::MpscQueue;

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
        assert_eq!(cmd.node_id, node);
        assert_eq!(cmd.param, "volume");
        assert!((cmd.value - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_lfo_servo() {
        let node = NodeId(1);
        let queue = Arc::new(MpscQueue::with_capacity(64));
        let mut control = PatchbayControl::new(queue);

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
        let queue = Arc::new(MpscQueue::with_capacity(64));
        let mut control = PatchbayControl::new(queue.clone());

        control.add_envelope("test_env", 0.1, 0.2, 0.7, 0.3, node, "gain", 0.0, 1.0);

        if let Some(_servo) = control.get_servo_mut("test_env") {
        }

        control.update(0.05);
        control.update(0.05);
    }
}
