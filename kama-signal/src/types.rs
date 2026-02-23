//! # Типы сигналов
//!
//! Предопределённые типы сигналов для использования в экосистеме Kama Audio.
//!
//! ## Основные типы
//!
//! - [`ParameterChanged`] - сигнал об изменении параметра узла
//! - [`ClockTick`] - тактовый сигнал от транспорта
//! - [`SystemEvent`] - системные события

//! Типы сигналов

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Базовый трейт сигнала
/// Маркерный трейт для типов, которые могут использоваться как сигналы.
///
/// Любой тип, реализующий этот трейт, может передаваться через `SignalBus`
/// и обрабатываться `SimpleSignalDispatcher`.
pub trait Signal: Send + Sync + 'static {}

/// Источник сигнала
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
/// Источник сигнала (для ParameterChanged).
///
/// Используется для отслеживания происхождения изменений параметров,
/// что позволяет реализовать защиту от обратной связи.
pub enum SignalSource {
    UserInterface,
    Automation,
    Midi { channel: u8, controller: u8 },
    Osc { address: String },
    Script,
    External,
}

/// Сигнал изменения параметра
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
/// Сигнал об изменении параметра узла.
///
/// Содержит всю необходимую информацию для обновления параметра.
pub struct ParameterChanged {
    pub node_id: String,
    pub parameter_id: String,
    pub value: f32,
    pub normalized_value: f32,
    pub timestamp: u64,
    pub source: SignalSource,
}

impl Signal for ParameterChanged {}

/// Сигнал тактового синхроимпульса
#[derive(Debug, Clone)]
/// Тактовый сигнал от транспорта.
///
/// Генерируется регулярно в аудиопотоке для синхронизации.
pub struct ClockTick {
    pub sample_pos: u64,
    pub samples_since_last: u32,
}

impl Signal for ClockTick {}

/// Системное событие
#[derive(Debug, Clone)]
/// Системные события.
///
/// Используются для уведомления об изменениях в графе,
/// состоянии транспорта и ошибках.
pub enum SystemEvent {
    /// Граф обработки изменился (добавлены/удалены узлы или соединения).
    GraphChanged,
    /// Транспорт запущен (воспроизведение начато).
    TransportStarted,
    /// Транспорт остановлен (воспроизведение приостановлено).
    TransportStopped,
    /// Ошибка в системе (с текстовым описанием).
    Error(String),
}

impl Signal for SystemEvent {}
