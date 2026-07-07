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

// Re-export control event types from rill-core (canonical home)
pub use rill_core::queues::control_event::{
    ControlEvent, EventPattern, MidiNoteKind, MidiTransportKind,
};

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
///
/// Also implements [`Automaton`] as a no-op — useful for mapping-only servos
/// where the automaton output is irrelevant.
#[derive(Debug, Clone, Default)]
pub struct NoAction;

impl Automaton for NoAction {
    type Internal = ();
    type Action = ();

    fn step(
        &self,
        _internal: &mut Self::Internal,
        _current: &ParamValue,
        _time: Time,
        _action: &Self::Action,
    ) -> ParamValue {
        ParamValue::Float(0.0)
    }

    fn initial_internal(&self) -> Self::Internal {}

    fn name(&self) -> &str {
        "NoAction"
    }
}

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
// 6.5. Control context — stateful MIDI controller aggregation
// =============================================================================

/// Per-servo mutable context for stateful control events.
///
/// Pitch bend and mod wheel values accumulate here. When a note-on
/// arrives, the servo composes the final frequency/amplitude from
/// the context and sends it directly (bypassing mappings).
#[derive(Debug, Clone)]
pub(crate) struct ControlContext {
    pitch_bend_semitones: f64,
    mod_wheel: f64,
    active_note: Option<u8>,
    active_velocity: Option<f32>,
}

impl Default for ControlContext {
    fn default() -> Self {
        Self {
            pitch_bend_semitones: 0.0,
            mod_wheel: 1.0,
            active_note: None,
            active_velocity: None,
        }
    }
}

/// Convert a MIDI note number to frequency in Hz (A4 = 440 Hz).
fn midi_note_to_freq(note: u8) -> f64 {
    440.0 * 2.0f64.powf((note as f64 - 69.0) / 12.0)
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
    /// Stateful control context for pitch bend / mod wheel / note tracking.
    pub(crate) control_ctx: ControlContext,
}

// =============================================================================
// 8. Servo — automaton-to-parameter bridge
// =============================================================================

/// Bridges an automaton to a graph parameter, stepping on every clock tick and
/// sending control commands to the signal graph.
///
/// Also accepts external control events (MIDI, OSC, CV/Gate) via
/// `CommandEnum::Control`, applying registered [`Mapping`]s to convert
/// them into `SetParameter` commands.
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
    /// Event-to-parameter mappings for sensor-driven control events.
    mappings: Vec<Mapping>,
    /// MIDI CC number for pitch bend (default 128 = pitch bend message).
    pitch_bend_cc: Option<u8>,
    /// Pitch bend range in semitones (±).
    pitch_bend_semis: f64,
    /// MIDI CC number for mod wheel (default 1).
    mod_wheel_cc: Option<u8>,
    /// String anchor name for rill-lang graph nodes. When set, parameter
    /// commands use `GraphSetParameter` instead of PortId-based `SetParameter`.
    target_anchor: Option<String>,
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
                control_ctx: ControlContext::default(),
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
            mappings: Vec::new(),
            pitch_bend_cc: None,
            pitch_bend_semis: 2.0,
            mod_wheel_cc: None,
            target_anchor: None,
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
            mappings,
            pitch_bend_cc,
            pitch_bend_semis,
            mod_wheel_cc,
            target_anchor,
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
        let pitch_cc = pitch_bend_cc;
        let pitch_semis = pitch_bend_semis;
        let mod_cc = mod_wheel_cc;
        let serv_id = id.clone();

        let s2 = s.clone();
        system.spawn_detached(
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
                            let sp = clock.sample_pos + clock.io_quantum as u64;
                            if let Some(ref anchor) = target_anchor {
                                gr.send(CommandEnum::GraphSetParameter {
                                    anchor: anchor.clone(),
                                    param: param.clone(),
                                    value: table[index].clone(),
                                });
                            } else {
                                let pid = ParameterId::new(&param).unwrap();
                                gr.send(CommandEnum::SetParameter(
                                    SetParameter::new(
                                        PortId::param(nid, 0),
                                        pid,
                                        table[index].clone(),
                                        SignalOrigin::Automaton(serv_id.clone()),
                                    )
                                    .with_sample_pos(sp),
                                ));
                            }
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

                        // Skip SetParameter when no target parameter configured
                        // (mapping-only servos with NoAction automaton).
                        if param.is_empty() {
                            return;
                        }

                        let sp = clock.sample_pos + clock.io_quantum as u64;
                        if let Some(ref anchor) = target_anchor {
                            gr.send(CommandEnum::GraphSetParameter {
                                anchor: anchor.clone(),
                                param: param.clone(),
                                value: ParamValue::Float(value as f32),
                            });
                        } else {
                            let pid = ParameterId::new(&param).unwrap();
                            gr.send(CommandEnum::SetParameter(
                                SetParameter::new(
                                    PortId::param(nid, 0),
                                    pid,
                                    ParamValue::Float(value as f32),
                                    SignalOrigin::Automaton(serv_id.clone()),
                                )
                                .with_sample_pos(sp),
                            ));
                        }
                    }
                    CommandEnum::Automaton(AutomatonCommand::SetEnabled { enabled, .. }) => {
                        s.lock().unwrap().enabled = enabled;
                    }
                    CommandEnum::Automaton(AutomatonCommand::Reset { .. }) => {
                        s.lock().unwrap().internal = a.reset();
                    }
                    CommandEnum::Automaton(AutomatonCommand::UiValue { value, .. }) => {
                        let mut state = s.lock().unwrap();
                        let should_send = match confl {
                            ConflictStrategy::TouchOverride => {
                                state.base = value;
                                state.frozen = true;
                                true
                            }
                            ConflictStrategy::BasePlusModulation => {
                                state.base = value;
                                false
                            }
                            ConflictStrategy::LastWriteWins => true,
                        };
                        if should_send {
                            if let Some(ref anchor) = target_anchor {
                                gr.send(CommandEnum::GraphSetParameter {
                                    anchor: anchor.clone(),
                                    param: param.clone(),
                                    value: ParamValue::Float(value as f32),
                                });
                            } else {
                                let pid = ParameterId::new(&param).unwrap();
                                gr.send(CommandEnum::SetParameter(SetParameter::new(
                                    PortId::param(nid, 0),
                                    pid,
                                    ParamValue::Float(value as f32),
                                    SignalOrigin::Automaton(serv_id.clone()),
                                )));
                            }
                        }
                    }
                    CommandEnum::Automaton(AutomatonCommand::UiRelease { .. }) => {
                        let mut state = s.lock().unwrap();
                        if state.frozen {
                            state.frozen = false;
                        }
                    }
                    CommandEnum::Control(event) => {
                        match &event {
                            // ── Pitch bend: update context, recalc if note active ──
                            ControlEvent::MidiControl {
                                controller,
                                normalized,
                                ..
                            } if Some(*controller) == pitch_cc => {
                                let mut state = s.lock().unwrap();
                                let semis = (*normalized as f64 - 0.5) * 2.0 * pitch_semis;
                                state.control_ctx.pitch_bend_semitones = semis;
                                drop(state);

                                let s3 = s.lock().unwrap();
                                if let (Some(note), Some(_vel)) =
                                    (s3.control_ctx.active_note, s3.control_ctx.active_velocity)
                                {
                                    let freq = midi_note_to_freq(note)
                                        * 2.0f64.powf(s3.control_ctx.pitch_bend_semitones / 12.0);
                                    let pid = ParameterId::new("frequency").unwrap();
                                    gr.send(CommandEnum::SetParameter(SetParameter::new(
                                        PortId::param(nid, 0),
                                        pid,
                                        ParamValue::Float(freq as f32),
                                        SignalOrigin::Automaton(serv_id.clone()),
                                    )));
                                }
                                drop(s3);
                            }
                            // ── Mod wheel: update context, recalc if note active ──
                            ControlEvent::MidiControl {
                                controller,
                                normalized,
                                ..
                            } if Some(*controller) == mod_cc => {
                                let mut state = s.lock().unwrap();
                                state.control_ctx.mod_wheel = *normalized as f64;
                                drop(state);

                                let s3 = s.lock().unwrap();
                                if let (Some(_note), Some(vel)) =
                                    (s3.control_ctx.active_note, s3.control_ctx.active_velocity)
                                {
                                    let amp = vel as f64 * s3.control_ctx.mod_wheel;
                                    let pid = ParameterId::new("amplitude").unwrap();
                                    gr.send(CommandEnum::SetParameter(SetParameter::new(
                                        PortId::param(nid, 0),
                                        pid,
                                        ParamValue::Float(amp as f32),
                                        SignalOrigin::Automaton(serv_id.clone()),
                                    )));
                                }
                                drop(s3);
                            }
                            // ── Note on: activate, compose from context ──
                            ControlEvent::MidiNote {
                                note,
                                velocity,
                                on: true,
                                ..
                            } if velocity > &0u8 => {
                                let vel_norm = *velocity as f32 / 127.0;
                                let mut state = s.lock().unwrap();
                                state.control_ctx.active_note = Some(*note);
                                state.control_ctx.active_velocity = Some(vel_norm);
                                let freq = midi_note_to_freq(*note)
                                    * 2.0f64.powf(state.control_ctx.pitch_bend_semitones / 12.0);
                                let amp = vel_norm as f64 * state.control_ctx.mod_wheel;
                                drop(state);

                                // Send frequency
                                let pid = ParameterId::new("frequency").unwrap();
                                gr.send(CommandEnum::SetParameter(SetParameter::new(
                                    PortId::param(nid, 0),
                                    pid,
                                    ParamValue::Float(freq as f32),
                                    SignalOrigin::Automaton(serv_id.clone()),
                                )));
                                // Send amplitude
                                let pid_amp = ParameterId::new("amplitude").unwrap();
                                gr.send(CommandEnum::SetParameter(SetParameter::new(
                                    PortId::param(nid, 0),
                                    pid_amp,
                                    ParamValue::Float(amp as f32),
                                    SignalOrigin::Automaton(serv_id.clone()),
                                )));
                            }
                            // ── Note off: deactivate, silence ──
                            ControlEvent::MidiNote { on: false, .. } => {
                                let mut state = s.lock().unwrap();
                                state.control_ctx.active_note = None;
                                state.control_ctx.active_velocity = None;
                                drop(state);

                                let pid = ParameterId::new("amplitude").unwrap();
                                gr.send(CommandEnum::SetParameter(SetParameter::new(
                                    PortId::param(nid, 0),
                                    pid,
                                    ParamValue::Float(0.0),
                                    SignalOrigin::Automaton(serv_id.clone()),
                                )));
                            }
                            // ── Fallback: iterate user-defined mappings ──
                            _ => {
                                let mut state = s.lock().unwrap();
                                for mapping in &mappings {
                                    if let Some(sp) = mapping.apply(&event) {
                                        match confl {
                                            ConflictStrategy::TouchOverride => {
                                                state.frozen = true;
                                                if let Some(nv) = event.normalized_value() {
                                                    state.base = nv as f64;
                                                }
                                                gr.send(CommandEnum::SetParameter(sp));
                                                break; // one mapping match — freeze + send
                                            }
                                            ConflictStrategy::BasePlusModulation => {
                                                if let Some(nv) = event.normalized_value() {
                                                    state.base = nv as f64;
                                                }
                                                // Don't send SetParameter — automaton
                                                // modulates around new base on next ClockTick.
                                            }
                                            ConflictStrategy::LastWriteWins => {
                                                gr.send(CommandEnum::SetParameter(sp));
                                            }
                                        }
                                    }
                                }
                            }
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

    /// Enable pitch bend tracking via MIDI CC.
    ///
    /// When a pitch bend CC arrives and a note is active, the servo
    /// recalculates frequency as `midi_to_freq(note) * 2^(bend/12)`.
    pub fn with_pitch_bend(mut self, cc: u8, semitones: f64) -> Self {
        self.pitch_bend_cc = Some(cc);
        self.pitch_bend_semis = semitones;
        self
    }

    /// Enable mod wheel tracking via MIDI CC.
    ///
    /// When a mod wheel CC arrives and a note is active, the servo
    /// recalculates amplitude as `(velocity/127) * mod_wheel`.
    pub fn with_mod_wheel(mut self, cc: u8) -> Self {
        self.mod_wheel_cc = Some(cc);
        self
    }

    /// Attaches sensor event mappings for [`Control`](CommandEnum::Control) dispatch.
    ///
    /// When the servo receives a `ControlEvent`, each mapping is checked;
    /// matching events produce `SetParameter` commands sent to the graph.
    pub fn with_mappings(mut self, mappings: Vec<Mapping>) -> Self {
        self.mappings = mappings;
        self
    }

    /// Set the control strategy — how the automaton affects the parameter value.
    ///
    /// - `Absolute` (default): automaton output [0,1] maps to [min,max].
    /// - `Modulation { depth }`: automaton output [-1,1] modulates around `base`.
    pub fn with_control(mut self, strategy: ControlStrategy) -> Self {
        self.control = strategy;
        self
    }

    /// Set the conflict resolution strategy — how UI/HID input interacts with
    /// automaton control for the same parameter.
    ///
    /// - `LastWriteWins` (default): both sources send independently; mailbox order.
    /// - `TouchOverride`: HID input freezes automaton until `UiRelease`.
    /// - `BasePlusModulation`: HID input sets the base value; automaton modulates around it.
    pub fn with_conflict(mut self, strategy: ConflictStrategy) -> Self {
        self.conflict = strategy;
        self
    }

    /// Set the string anchor for rill-lang graph targeting.
    ///
    /// When set, parameter commands use `GraphSetParameter` with this anchor
    /// instead of PortId-based `SetParameter`.
    pub fn with_anchor(mut self, anchor: String) -> Self {
        self.target_anchor = Some(anchor);
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
