//! Signal and command types for queues.
//!
//! This module defines all command types that can be sent through queues
//! between Rill components. Each command type represents a specific
//! action or event in the system.
//!
//! ## Command hierarchy
//!
//! - `CommandEnum` — top-level enum wrapping all command variants
//! - `SetParameter` — parameter change for a signal graph node
//! - `AutomatonCommand` — automaton control
//! - `SensorCommand` — sensor control
//! - `ServoCommand` — servo control
//!
//! ## Example
//!
//! ```no_run
//! use rill_core::queues::*;
//! use rill_core::traits::*;
//!
//! let node = NodeId(1);
//! let port = PortId::control_in(node, 0);
//! let param = ParameterId::new("gain").unwrap();
//! let cmd = SetParameter::new(port, param, ParamValue::Float(0.5), SignalOrigin::Automaton("lfo".into()));
//! // Send via ActorRef<SetParameter> or MpscQueue<SetParameter>
//! ```

use super::command::Command;
use super::control_event::ControlEvent;
use crate::time::ClockTick;
use crate::traits::{ParamValue, ParameterId, PortId};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

//==============================================================================
// SignalOrigin — signal source
//==============================================================================

/// Origin of a signal or command.
///
/// Used for tracking command provenance, feedback-loop prevention,
/// and telemetry attribution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SignalOrigin {
    /// Command from an automaton (LFO, envelope, sequencer).
    Automaton(String),
    /// Command from a sensor (physical input device).
    Sensor(String),
    /// Command from a servo (physical output device).
    Servo(String),
    /// Command from an external source (OSC, etc.).
    External(String),
    /// Manual user interaction (UI slider, button, etc.).
    Manual,
    /// Command from a script.
    Script,
}

impl SignalOrigin {
    /// Return the human-readable name of this source.
    pub fn name(&self) -> &str {
        match self {
            SignalOrigin::Automaton(name) => name,
            SignalOrigin::Sensor(name) => name,
            SignalOrigin::Servo(name) => name,
            SignalOrigin::External(name) => name,
            SignalOrigin::Manual => "manual",
            SignalOrigin::Script => "script",
        }
    }

    /// Return the type category of this source (e.g. "automaton", "sensor").
    pub fn kind(&self) -> &'static str {
        match self {
            SignalOrigin::Automaton(_) => "automaton",
            SignalOrigin::Sensor(_) => "sensor",
            SignalOrigin::Servo(_) => "servo",
            SignalOrigin::External(_) => "external",
            SignalOrigin::Manual => "manual",
            SignalOrigin::Script => "script",
        }
    }
}

impl fmt::Display for SignalOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignalOrigin::Automaton(name) => write!(f, "⚙️ {}", name),
            SignalOrigin::Sensor(name) => write!(f, "👁️ {}", name),
            SignalOrigin::Servo(name) => write!(f, "🦾 {}", name),
            SignalOrigin::External(name) => write!(f, "🌍 {}", name),
            SignalOrigin::Manual => write!(f, "👤 manual"),
            SignalOrigin::Script => write!(f, "📜 script"),
        }
    }
}

// ===== SetParameter =====

/// Command to change a parameter value on a signal graph node.
#[derive(Debug, Clone)]
pub struct SetParameter {
    /// Target port.
    pub port: PortId,
    /// Target parameter identifier.
    pub parameter: ParameterId,
    /// New parameter value.
    pub value: ParamValue,
    /// Origin of this command.
    pub source: SignalOrigin,
    /// Unix timestamp (microseconds).
    pub timestamp: u64,
    /// Optional sample-accurate application time (absolute sample position).
    ///
    /// When `Some(pos)`, the graph applies this change during the processing
    /// block whose sample range contains `pos`, rather than immediately on
    /// drain. This lets tick-driven producers (sequencers, servos) place
    /// parameter changes at exact sample positions instead of being subject to
    /// how the backend batches blocks per I/O callback. `None` = apply as soon
    /// as it is drained (legacy behaviour).
    pub sample_pos: Option<u64>,
}

impl SetParameter {
    /// Create a new parameter-change command with the current timestamp.
    pub fn new(
        port: PortId,
        parameter: ParameterId,
        value: ParamValue,
        source: SignalOrigin,
    ) -> Self {
        Self {
            port,
            parameter,
            value,
            source,
            timestamp: Self::now(),
            sample_pos: None,
        }
    }

    /// Create a new parameter-change command with an explicit timestamp.
    pub fn with_timestamp(
        port: PortId,
        parameter: ParameterId,
        value: ParamValue,
        source: SignalOrigin,
        timestamp: u64,
    ) -> Self {
        Self {
            port,
            parameter,
            value,
            source,
            timestamp,
            sample_pos: None,
        }
    }

    /// Set the sample-accurate application time (absolute sample position).
    pub fn with_sample_pos(mut self, sample_pos: u64) -> Self {
        self.sample_pos = Some(sample_pos);
        self
    }

    /// Return the current Unix time in microseconds.
    pub fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64
    }
}

impl PartialEq for SetParameter {
    fn eq(&self, other: &Self) -> bool {
        self.port == other.port
            && self.parameter == other.parameter
            && self.value == other.value
            && self.source == other.source
    }
}

impl fmt::Display for SetParameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} → {}::{} = {:?}",
            self.timestamp, self.source, self.port, self.parameter, self.value
        )
    }
}

// Implement the Command trait for SetParameter
impl Command for SetParameter {}

// ===== AutomatonCommand =====

/// Commands for controlling automatons (LFOs, envelopes, sequencers).
#[derive(Debug, Clone)]
pub enum AutomatonCommand {
    /// Enable or disable an automaton by ID.
    SetEnabled {
        /// Automaton identifier.
        id: String,
        /// Whether the automaton should be enabled.
        enabled: bool,
    },
    /// Set a named parameter on an automaton.
    SetParameter {
        /// Automaton identifier.
        id: String,
        /// Parameter name.
        name: String,
        /// Parameter value.
        value: f32,
    },
    /// Reset an automaton to its initial state.
    Reset {
        /// Automaton identifier.
        id: String,
    },
    /// Connect an automaton output to another automaton input.
    Connect {
        /// Source automaton identifier.
        from: String,
        /// Destination automaton identifier.
        to: String,
        /// Connection gain.
        gain: f32,
    },
    /// Disconnect two automatons.
    Disconnect {
        /// Source automaton identifier.
        from: String,
        /// Destination automaton identifier.
        to: String,
    },
    /// Create a new automaton instance.
    Create {
        /// Automaton type (e.g. "lfo", "envelope").
        kind: String,
        /// New automaton identifier.
        id: String,
        /// Initial parameter values.
        params: Vec<(String, f32)>,
    },
    /// Destroy an automaton by ID.
    Destroy {
        /// Automaton identifier to remove.
        id: String,
    },
    /// Wake the automaton to process a clock tick (no payload required).
    Wake {
        /// Automaton identifier.
        id: String,
    },
    /// Set a value from UI input for conflict resolution.
    UiValue {
        /// Automaton identifier.
        id: String,
        /// Raw value from UI.
        value: f64,
    },
    /// Release UI control (unfreeze in TouchOverride mode).
    UiRelease {
        /// Automaton identifier.
        id: String,
    },
}

impl AutomatonCommand {
    /// Return the target automaton ID, if applicable.
    pub fn automaton_id(&self) -> Option<&str> {
        match self {
            AutomatonCommand::SetEnabled { id, .. } => Some(id),
            AutomatonCommand::SetParameter { id, .. } => Some(id),
            AutomatonCommand::Reset { id } => Some(id),
            AutomatonCommand::Connect { from, to: _to, .. } => Some(from),
            AutomatonCommand::Disconnect { from, to: _to } => Some(from),
            AutomatonCommand::Create { id, .. } => Some(id),
            AutomatonCommand::Destroy { id } => Some(id),
            AutomatonCommand::Wake { id } => Some(id),
            AutomatonCommand::UiValue { id, .. } => Some(id),
            AutomatonCommand::UiRelease { id } => Some(id),
        }
    }
}

impl fmt::Display for AutomatonCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutomatonCommand::SetEnabled { id, enabled } => {
                write!(f, "Automaton[{}] set_enabled({})", id, enabled)
            }
            AutomatonCommand::SetParameter { id, name, value } => {
                write!(f, "Automaton[{}] set_param({}={:.2})", id, name, value)
            }
            AutomatonCommand::Reset { id } => {
                write!(f, "Automaton[{}] reset()", id)
            }
            AutomatonCommand::Connect { from, to, gain } => {
                write!(f, "Automaton connect {} → {} gain={:.2}", from, to, gain)
            }
            AutomatonCommand::Disconnect { from, to } => {
                write!(f, "Automaton disconnect {} → {}", from, to)
            }
            AutomatonCommand::Create { kind, id, params } => {
                write!(
                    f,
                    "Automaton create {} as {} with {} params",
                    kind,
                    id,
                    params.len()
                )
            }
            AutomatonCommand::Destroy { id } => {
                write!(f, "Automaton destroy {}", id)
            }
            AutomatonCommand::Wake { id } => {
                write!(f, "Automaton[{}] wake(tick)", id)
            }
            AutomatonCommand::UiValue { id, value } => {
                write!(f, "Automaton[{}] ui_value({:.2})", id, value)
            }
            AutomatonCommand::UiRelease { id } => {
                write!(f, "Automaton[{}] ui_release()", id)
            }
        }
    }
}

impl Command for AutomatonCommand {}

// ===== SensorCommand =====

/// Type of sensor calibration to perform.
#[derive(Debug, Clone)]
pub enum CalibrationKind {
    /// Automatically determine min/max from signal range.
    Auto,
    /// Set the current sensor reading as the minimum value.
    SetCurrentAsMin,
    /// Set the current sensor reading as the maximum value.
    SetCurrentAsMax,
    /// Reset calibration to factory defaults.
    Reset,
}

impl fmt::Display for CalibrationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CalibrationKind::Auto => write!(f, "auto"),
            CalibrationKind::SetCurrentAsMin => write!(f, "set_min"),
            CalibrationKind::SetCurrentAsMax => write!(f, "set_max"),
            CalibrationKind::Reset => write!(f, "reset"),
        }
    }
}

/// Commands for controlling sensors (physical input devices).
#[derive(Debug, Clone)]
pub enum SensorCommand {
    /// Start listening to a sensor data source.
    StartListening {
        /// Sensor identifier.
        id: String,
        /// Data source to listen to.
        source: String,
    },
    /// Stop listening to a sensor.
    StopListening {
        /// Sensor identifier.
        id: String,
    },
    /// Set sensor sensitivity.
    SetSensitivity {
        /// Sensor identifier.
        id: String,
        /// Sensitivity value.
        value: f32,
    },
    /// Calibrate a sensor.
    Calibrate {
        /// Sensor identifier.
        id: String,
        /// Calibration type.
        kind: CalibrationKind,
    },
    /// Enable or disable a sensor.
    SetEnabled {
        /// Sensor identifier.
        id: String,
        /// Whether the sensor should be enabled.
        enabled: bool,
    },
}

impl SensorCommand {
    /// Return the target sensor ID.
    pub fn sensor_id(&self) -> &str {
        match self {
            SensorCommand::StartListening { id, .. } => id,
            SensorCommand::StopListening { id } => id,
            SensorCommand::SetSensitivity { id, .. } => id,
            SensorCommand::Calibrate { id, .. } => id,
            SensorCommand::SetEnabled { id, .. } => id,
        }
    }
}

impl fmt::Display for SensorCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SensorCommand::StartListening { id, source } => {
                write!(f, "Sensor[{}] start listening to {}", id, source)
            }
            SensorCommand::StopListening { id } => {
                write!(f, "Sensor[{}] stop listening", id)
            }
            SensorCommand::SetSensitivity { id, value } => {
                write!(f, "Sensor[{}] set sensitivity to {:.2}", id, value)
            }
            SensorCommand::Calibrate { id, kind } => {
                write!(f, "Sensor[{}] calibrate {}", id, kind)
            }
            SensorCommand::SetEnabled { id, enabled } => {
                write!(f, "Sensor[{}] set enabled({})", id, enabled)
            }
        }
    }
}

impl Command for SensorCommand {}

// ===== ServoCommand =====

/// Mapping function type for servo output value transformation.
#[derive(Debug, Clone)]
pub enum MappingType {
    /// Linear mapping (identity).
    Linear,
    /// Exponential mapping.
    Exponential,
    /// Logarithmic mapping.
    Logarithmic,
    /// Inverted (reverse) mapping.
    Inverted,
    /// Custom named mapping function.
    Custom(String),
}

impl fmt::Display for MappingType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MappingType::Linear => write!(f, "linear"),
            MappingType::Exponential => write!(f, "exponential"),
            MappingType::Logarithmic => write!(f, "logarithmic"),
            MappingType::Inverted => write!(f, "inverted"),
            MappingType::Custom(s) => write!(f, "custom({})", s),
        }
    }
}

/// Commands for controlling servos (physical output devices).
#[derive(Debug, Clone)]
pub enum ServoCommand {
    /// Bind a servo to follow an automaton output.
    BindToAutomaton {
        /// Servo identifier.
        servo_id: String,
        /// Automaton identifier to bind to.
        automaton_id: String,
    },
    /// Bind a servo directly to a signal graph parameter.
    BindToParameter {
        /// Servo identifier.
        servo_id: String,
        /// Target port.
        port: PortId,
        /// Target parameter.
        parameter: ParameterId,
    },
    /// Unbind a servo from all sources.
    Unbind {
        /// Servo identifier.
        servo_id: String,
    },
    /// Set the output range of a servo.
    SetRange {
        /// Servo identifier.
        servo_id: String,
        /// Minimum output value.
        min: f32,
        /// Maximum output value.
        max: f32,
    },
    /// Set the value mapping function for a servo.
    SetMapping {
        /// Servo identifier.
        servo_id: String,
        /// Mapping type.
        mapping: MappingType,
    },
    /// Enable or disable a servo.
    SetEnabled {
        /// Servo identifier.
        servo_id: String,
        /// Whether the servo should be enabled.
        enabled: bool,
    },
}

impl ServoCommand {
    /// Return the target servo ID.
    pub fn servo_id(&self) -> &str {
        match self {
            ServoCommand::BindToAutomaton { servo_id, .. } => servo_id,
            ServoCommand::BindToParameter { servo_id, .. } => servo_id,
            ServoCommand::Unbind { servo_id } => servo_id,
            ServoCommand::SetRange { servo_id, .. } => servo_id,
            ServoCommand::SetMapping { servo_id, .. } => servo_id,
            ServoCommand::SetEnabled { servo_id, .. } => servo_id,
        }
    }
}

impl fmt::Display for ServoCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ServoCommand::BindToAutomaton {
                servo_id,
                automaton_id,
            } => {
                write!(f, "Servo[{}] bind to automaton {}", servo_id, automaton_id)
            }
            ServoCommand::BindToParameter {
                servo_id,
                port,
                parameter,
            } => {
                write!(f, "Servo[{}] bind to {}::{}", servo_id, port, parameter)
            }
            ServoCommand::Unbind { servo_id } => {
                write!(f, "Servo[{}] unbind", servo_id)
            }
            ServoCommand::SetRange { servo_id, min, max } => {
                write!(f, "Servo[{}] set range [{}, {}]", servo_id, min, max)
            }
            ServoCommand::SetMapping { servo_id, mapping } => {
                write!(f, "Servo[{}] set mapping {}", servo_id, mapping)
            }
            ServoCommand::SetEnabled { servo_id, enabled } => {
                write!(f, "Servo[{}] set enabled({})", servo_id, enabled)
            }
        }
    }
}

impl Command for ServoCommand {}

// ===== CommandType (formerly Command) — common command type =====

/// Runtime command type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandType {
    /// Parameter change command.
    SetParameter,
    /// Automaton control command.
    Automaton,
    /// Sensor control command.
    Sensor,
    /// Servo control command.
    Servo,
    /// Clock tick.
    ClockTick,
    /// Stop command — shuts down the actor's I/O loop.
    Stop,
    /// System command.
    System,
    /// Control event from a sensor.
    Control,
}

impl fmt::Display for CommandType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandType::SetParameter => write!(f, "SetParameter"),
            CommandType::Automaton => write!(f, "Automaton"),
            CommandType::Sensor => write!(f, "Sensor"),
            CommandType::Servo => write!(f, "Servo"),
            CommandType::ClockTick => write!(f, "ClockTick"),
            CommandType::Stop => write!(f, "Stop"),
            CommandType::System => write!(f, "System"),
            CommandType::Control => write!(f, "Control"),
        }
    }
}

/// Universal command enum combining all possible command types.
///
/// Useful when a single queue must transport multiple command types,
/// or when the command type is not known ahead of time.
#[derive(Debug, Clone)]
pub enum CommandEnum {
    /// Parameter change command.
    SetParameter(SetParameter),
    /// Automaton control command.
    Automaton(AutomatonCommand),
    /// Sensor control command.
    Sensor(SensorCommand),
    /// Servo control command.
    Servo(ServoCommand),
    /// Clock tick — sent from Graph to Patchbay each processing block.
    ClockTick(ClockTick),
    /// Control event — decoded by a sensor, dispatched to a servo
    /// for mapping to a graph parameter.
    Control(ControlEvent),
    /// Stop command — shuts down the actor's I/O loop.
    Stop,
    /// System-level command with opaque payload.
    System {
        /// System command kind.
        kind: String,
        /// Opaque command data.
        data: Vec<u8>,
    },
    /// Graph anchor-based parameter change (targets a named anchor in RillGraphEngine).
    /// Bypasses PortId — the engine routes by anchor name internally.
    GraphSetParameter {
        /// Anchor name identifying the target node.
        anchor: String,
        /// Parameter name.
        param: String,
        /// New parameter value.
        value: ParamValue,
    },
}

impl CommandEnum {
    /// Return the runtime type tag of this command.
    pub fn command_type(&self) -> CommandType {
        match self {
            CommandEnum::SetParameter(_) => CommandType::SetParameter,
            CommandEnum::GraphSetParameter { .. } => CommandType::SetParameter,
            CommandEnum::Automaton(_) => CommandType::Automaton,
            CommandEnum::Sensor(_) => CommandType::Sensor,
            CommandEnum::Servo(_) => CommandType::Servo,
            CommandEnum::ClockTick(_) => CommandType::ClockTick,
            CommandEnum::Stop => CommandType::Stop,
            CommandEnum::System { .. } => CommandType::System,
            CommandEnum::Control(_) => CommandType::Control,
        }
    }

    /// If this is a `SetParameter` command, return the target `NodeId`.
    pub fn target_node_id(&self) -> Option<crate::traits::NodeId> {
        match self {
            CommandEnum::SetParameter(cmd) => Some(cmd.port.node_id()),
            _ => None,
        }
    }

    /// Return the timestamp if the command carries one.
    pub fn timestamp(&self) -> Option<u64> {
        match self {
            CommandEnum::SetParameter(cmd) => Some(cmd.timestamp),
            _ => None,
        }
    }

    /// Try to downcast to `SetParameter`.
    pub fn as_set_parameter(&self) -> Option<&SetParameter> {
        match self {
            CommandEnum::SetParameter(cmd) => Some(cmd),
            _ => None,
        }
    }

    /// Try to downcast to `AutomatonCommand`.
    pub fn as_automaton(&self) -> Option<&AutomatonCommand> {
        match self {
            CommandEnum::Automaton(cmd) => Some(cmd),
            _ => None,
        }
    }

    /// Try to downcast to `SensorCommand`.
    pub fn as_sensor(&self) -> Option<&SensorCommand> {
        match self {
            CommandEnum::Sensor(cmd) => Some(cmd),
            _ => None,
        }
    }

    /// Try to downcast to `ServoCommand`.
    pub fn as_servo(&self) -> Option<&ServoCommand> {
        match self {
            CommandEnum::Servo(cmd) => Some(cmd),
            _ => None,
        }
    }

    /// Try to downcast to `ClockTick`.
    pub fn as_clock_tick(&self) -> Option<&ClockTick> {
        match self {
            CommandEnum::ClockTick(tick) => Some(tick),
            _ => None,
        }
    }
}

impl fmt::Display for CommandEnum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandEnum::SetParameter(cmd) => write!(f, "{}", cmd),
            CommandEnum::Automaton(cmd) => write!(f, "{}", cmd),
            CommandEnum::Sensor(cmd) => write!(f, "{}", cmd),
            CommandEnum::Servo(cmd) => write!(f, "{}", cmd),
            CommandEnum::ClockTick(tick) => write!(
                f,
                "ClockTick(pos={}, dt={}samp)",
                tick.sample_pos, tick.samples_since_last,
            ),
            CommandEnum::Stop => write!(f, "Stop"),
            CommandEnum::System { kind, data } => {
                write!(f, "System({kind}, {} bytes)", data.len())
            }
            CommandEnum::Control(event) => {
                write!(f, "ControlEvent({event:?})")
            }
            CommandEnum::GraphSetParameter {
                anchor,
                param,
                value,
            } => {
                write!(f, "GraphSetParameter({anchor}::{param} = {value:?})")
            }
        }
    }
}

// Implement the Command trait for CommandEnum (used via actor mailboxes).
impl Command for CommandEnum {}

// ===== Conversions =====

/// Marker trait for types that can be converted into a command.
pub trait ToCommand: Send + 'static {
    /// The command type this type converts into.
    type Command: Into<CommandEnum>;

    /// Convert self into a command.
    fn to_command(self) -> Self::Command;
}

/// Marker trait for types that can be constructed from a command.
pub trait FromCommand: Sized {
    /// The command type this type is constructed from.
    type Command: TryInto<Self> + Clone;

    /// Try to construct from a command.
    fn from_command(cmd: Self::Command) -> Option<Self>;
}

impl From<SetParameter> for CommandEnum {
    fn from(cmd: SetParameter) -> Self {
        CommandEnum::SetParameter(cmd)
    }
}

impl From<AutomatonCommand> for CommandEnum {
    fn from(cmd: AutomatonCommand) -> Self {
        CommandEnum::Automaton(cmd)
    }
}

impl From<SensorCommand> for CommandEnum {
    fn from(cmd: SensorCommand) -> Self {
        CommandEnum::Sensor(cmd)
    }
}

impl From<ServoCommand> for CommandEnum {
    fn from(cmd: ServoCommand) -> Self {
        CommandEnum::Servo(cmd)
    }
}

impl TryFrom<CommandEnum> for SetParameter {
    type Error = ();

    fn try_from(cmd: CommandEnum) -> Result<Self, Self::Error> {
        match cmd {
            CommandEnum::SetParameter(cmd) => Ok(cmd),
            _ => Err(()),
        }
    }
}

impl TryFrom<CommandEnum> for AutomatonCommand {
    type Error = ();

    fn try_from(cmd: CommandEnum) -> Result<Self, Self::Error> {
        match cmd {
            CommandEnum::Automaton(cmd) => Ok(cmd),
            _ => Err(()),
        }
    }
}

impl TryFrom<CommandEnum> for SensorCommand {
    type Error = ();

    fn try_from(cmd: CommandEnum) -> Result<Self, Self::Error> {
        match cmd {
            CommandEnum::Sensor(cmd) => Ok(cmd),
            _ => Err(()),
        }
    }
}

impl TryFrom<CommandEnum> for ServoCommand {
    type Error = ();

    fn try_from(cmd: CommandEnum) -> Result<Self, Self::Error> {
        match cmd {
            CommandEnum::Servo(cmd) => Ok(cmd),
            _ => Err(()),
        }
    }
}
