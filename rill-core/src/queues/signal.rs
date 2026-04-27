//! Типы сигналов и команд для очередей
//!
//! Этот модуль определяет все типы команд, которые могут передаваться
//! через очереди между компонентами Rill. Каждый тип команды
//! представляет определенное действие или событие в системе.
//!
//! ## Иерархия команд
//!
//! - `Command` — общий тип-перечисление всех возможных команд
//! - `SetParameter` — изменение параметра в AudioGraph
//! - `AutomatonCommand` — управление автоматами
//! - `SensorCommand` — управление сенсорами
//! - `ServoCommand` — управление серво
//!
//! ## Пример
//!
//! ```rust
//! use rill_core::queues::*;
//! use rill_core::traits::*;
//! #
//! // Создаем очередь команд
//! let queue: CommandQueue<CommandEnum> = CommandQueue::new("audio-control", 1024);
//!
//! // Создаем идентификаторы
//! let node = NodeId(1);
//! let port = PortId::control_in(node, 0);
//! let param = ParameterId::new("gain").unwrap();
//!
//! // Где-то в мире автоматов
//! let cmd = SetParameter::new(port, param, 0.5, SignalSource::Automaton("lfo".into()));
//! queue.send(CommandEnum::SetParameter(cmd)).unwrap();
//!
//! // Где-то в звуковом мире
//! while let Ok(cmd_enum) = queue.try_recv() {
//!     if let CommandEnum::SetParameter(cmd) = cmd_enum {
//!         // apply_parameter(cmd); // функция должна быть определена
//!     }
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use super::command::Command;
use crate::traits::{ParameterId, PortId};
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

//==============================================================================
// SignalSource — источник сигнала
//==============================================================================

/// Источник сигнала (откуда пришла команда)
///
/// Используется для отслеживания происхождения команд и телеметрии.
/// Позволяет реализовать защиту от обратной связи и отладку.
/// Типы сигналов и команд для очередей
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SignalSource {
    Automaton(String),
    Sensor(String),
    Servo(String),
    External(String),
    Manual,
    Script,
}

impl SignalSource {
    pub fn name(&self) -> &str {
        match self {
            SignalSource::Automaton(name) => name,
            SignalSource::Sensor(name) => name,
            SignalSource::Servo(name) => name,
            SignalSource::External(name) => name,
            SignalSource::Manual => "manual",
            SignalSource::Script => "script",
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            SignalSource::Automaton(_) => "automaton",
            SignalSource::Sensor(_) => "sensor",
            SignalSource::Servo(_) => "servo",
            SignalSource::External(_) => "external",
            SignalSource::Manual => "manual",
            SignalSource::Script => "script",
        }
    }
}

impl fmt::Display for SignalSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SignalSource::Automaton(name) => write!(f, "⚙️ {}", name),
            SignalSource::Sensor(name) => write!(f, "👁️ {}", name),
            SignalSource::Servo(name) => write!(f, "🦾 {}", name),
            SignalSource::External(name) => write!(f, "🌍 {}", name),
            SignalSource::Manual => write!(f, "👤 manual"),
            SignalSource::Script => write!(f, "📜 script"),
        }
    }
}

// ===== SetParameter =====

/// Команда изменения параметра
#[derive(Debug, Clone)]
pub struct SetParameter {
    pub port: PortId,
    pub parameter: ParameterId,
    pub value: f32,
    pub source: SignalSource,
    pub timestamp: u64,
}

impl SetParameter {
    pub fn new(port: PortId, parameter: ParameterId, value: f32, source: SignalSource) -> Self {
        Self {
            port,
            parameter,
            value,
            source,
            timestamp: Self::now(),
        }
    }

    pub fn with_timestamp(
        port: PortId,
        parameter: ParameterId,
        value: f32,
        source: SignalSource,
        timestamp: u64,
    ) -> Self {
        Self {
            port,
            parameter,
            value,
            source,
            timestamp,
        }
    }

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
            && (self.value - other.value).abs() < f32::EPSILON
            && self.source == other.source
    }
}

impl fmt::Display for SetParameter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {} → {}::{} = {:.3}",
            self.timestamp, self.source, self.port, self.parameter, self.value
        )
    }
}

// Реализуем трейт Command для SetParameter
impl Command for SetParameter {}

// ===== AutomatonCommand =====

#[derive(Debug, Clone)]
pub enum AutomatonCommand {
    SetEnabled {
        id: String,
        enabled: bool,
    },
    SetParameter {
        id: String,
        name: String,
        value: f32,
    },
    Reset {
        id: String,
    },
    Connect {
        from: String,
        to: String,
        gain: f32,
    },
    Disconnect {
        from: String,
        to: String,
    },
    Create {
        kind: String,
        id: String,
        params: Vec<(String, f32)>,
    },
    Destroy {
        id: String,
    },
}

impl AutomatonCommand {
    pub fn automaton_id(&self) -> Option<&str> {
        match self {
            AutomatonCommand::SetEnabled { id, .. } => Some(id),
            AutomatonCommand::SetParameter { id, .. } => Some(id),
            AutomatonCommand::Reset { id } => Some(id),
            AutomatonCommand::Connect { from, to: _to, .. } => Some(from),
            AutomatonCommand::Disconnect { from, to: _to } => Some(from),
            AutomatonCommand::Create { id, .. } => Some(id),
            AutomatonCommand::Destroy { id } => Some(id),
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
        }
    }
}

impl Command for AutomatonCommand {}

// ===== SensorCommand =====

#[derive(Debug, Clone)]
pub enum CalibrationKind {
    Auto,
    SetCurrentAsMin,
    SetCurrentAsMax,
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

#[derive(Debug, Clone)]
pub enum SensorCommand {
    StartListening { id: String, source: String },
    StopListening { id: String },
    SetSensitivity { id: String, value: f32 },
    Calibrate { id: String, kind: CalibrationKind },
    SetEnabled { id: String, enabled: bool },
}

impl SensorCommand {
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

#[derive(Debug, Clone)]
pub enum MappingType {
    Linear,
    Exponential,
    Logarithmic,
    Inverted,
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

#[derive(Debug, Clone)]
pub enum ServoCommand {
    BindToAutomaton {
        servo_id: String,
        automaton_id: String,
    },
    BindToParameter {
        servo_id: String,
        port: PortId,
        parameter: ParameterId,
    },
    Unbind {
        servo_id: String,
    },
    SetRange {
        servo_id: String,
        min: f32,
        max: f32,
    },
    SetMapping {
        servo_id: String,
        mapping: MappingType,
    },
    SetEnabled {
        servo_id: String,
        enabled: bool,
    },
}

impl ServoCommand {
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

// ===== CommandType (бывшее Command) — общий тип команды =====

/// Тип команды (для идентификации в рантайме)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandType {
    /// Изменение параметра
    SetParameter,
    /// Управление автоматом
    Automaton,
    /// Управление сенсором
    Sensor,
    /// Управление серво
    Servo,
    /// Системная команда
    System,
}

impl fmt::Display for CommandType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CommandType::SetParameter => write!(f, "SetParameter"),
            CommandType::Automaton => write!(f, "Automaton"),
            CommandType::Sensor => write!(f, "Sensor"),
            CommandType::Servo => write!(f, "Servo"),
            CommandType::System => write!(f, "System"),
        }
    }
}

/// Общий тип команды, объединяющий все возможные команды
///
/// Полезен, когда нужно использовать одну очередь для всех типов команд,
/// или когда тип команды неизвестен заранее.
#[derive(Debug, Clone)]
pub enum CommandEnum {
    /// Изменение параметра
    SetParameter(SetParameter),
    /// Управление автоматом
    Automaton(AutomatonCommand),
    /// Управление сенсором
    Sensor(SensorCommand),
    /// Управление серво
    Servo(ServoCommand),
    /// Системная команда
    System { kind: String, data: Vec<u8> },
}

impl CommandEnum {
    /// Получить тип команды
    pub fn command_type(&self) -> CommandType {
        match self {
            CommandEnum::SetParameter(_) => CommandType::SetParameter,
            CommandEnum::Automaton(_) => CommandType::Automaton,
            CommandEnum::Sensor(_) => CommandType::Sensor,
            CommandEnum::Servo(_) => CommandType::Servo,
            CommandEnum::System { .. } => CommandType::System,
        }
    }

    /// Получить временную метку (если есть)
    pub fn timestamp(&self) -> Option<u64> {
        match self {
            CommandEnum::SetParameter(cmd) => Some(cmd.timestamp),
            _ => None,
        }
    }

    /// Попытаться преобразовать в SetParameter
    pub fn as_set_parameter(&self) -> Option<&SetParameter> {
        match self {
            CommandEnum::SetParameter(cmd) => Some(cmd),
            _ => None,
        }
    }

    /// Попытаться преобразовать в AutomatonCommand
    pub fn as_automaton(&self) -> Option<&AutomatonCommand> {
        match self {
            CommandEnum::Automaton(cmd) => Some(cmd),
            _ => None,
        }
    }

    /// Попытаться преобразовать в SensorCommand
    pub fn as_sensor(&self) -> Option<&SensorCommand> {
        match self {
            CommandEnum::Sensor(cmd) => Some(cmd),
            _ => None,
        }
    }

    /// Попытаться преобразовать в ServoCommand
    pub fn as_servo(&self) -> Option<&ServoCommand> {
        match self {
            CommandEnum::Servo(cmd) => Some(cmd),
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
            CommandEnum::System { kind, data } => {
                write!(f, "System[{}] ({} bytes)", kind, data.len())
            }
        }
    }
}

// Реализуем трейт Command для CommandEnum, чтобы его можно было использовать в CommandQueue
impl Command for CommandEnum {}

// ===== Преобразования =====

/// Маркерный трейт для типов, которые могут быть преобразованы в команду
pub trait ToCommand: Send + 'static {
    /// Тип команды, в которую преобразуется
    type Command: Into<CommandEnum>;

    /// Преобразовать в команду
    fn to_command(self) -> Self::Command;
}

/// Маркерный трейт для типов, которые могут быть созданы из команды
pub trait FromCommand: Sized {
    /// Тип команды, из которой создается
    type Command: TryInto<Self> + Clone;

    /// Попытаться создать из команды
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
