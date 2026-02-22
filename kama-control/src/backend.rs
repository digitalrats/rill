//! # Бэкенды контроллеров
//! 
//! Определяет общий интерфейс [`ControlBackend`] для всех типов контроллеров
//! и базовые типы событий [`ControlEvent`].

use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::error::ControlResult;

/// Событие контроллера
#[derive(Debug, Clone, PartialEq)]
pub enum ControlEvent {
    /// Кнопка
    /// Событие кнопки (нажата/отпущена).
    Button {
        id: u32,
        pressed: bool,
    },
    
    /// Поворотная ручка (энкодер)
    /// Событие поворотной ручки (энкодер).
    Knob {
        id: u32,
        value: f32,           // 0.0 - 1.0
        normalized: f32,       // то же, для совместимости
    },
    
    /// Фейдер (линейный ползунок)
    /// Событие линейного фейдера.
    Fader {
        id: u32,
        value: f32,           // 0.0 - 1.0
        normalized: f32,
    },
    
    /// MIDI сообщение
    /// Сырое MIDI сообщение.
    /// MIDI бэкенд.
    Midi {
        channel: u8,          // 0-15
        message: Vec<u8>,
    },
    
    /// MIDI Control Change (специализированное)
    /// MIDI Control Change сообщение.
    /// MIDI бэкенд.
    MidiControl {
        channel: u8,
        controller: u8,
        value: u8,            // 0-127
        normalized: f32,      // 0.0 - 1.0
    },
    
    /// MIDI Note (для клавиатур)
    /// MIDI Note On/Off сообщение.
    /// MIDI бэкенд.
    MidiNote {
        channel: u8,
        note: u8,
        velocity: u8,
        on: bool,
    },
    
    /// OSC сообщение
    /// OSC сообщение.
    /// OSC бэкенд.
    Osc {
        address: String,
        args: Vec<f32>,
    },
    
    /// Пользовательское событие
    /// Пользовательское событие.
    /// Пользовательский бэкенд.
    Custom(String, Vec<f32>),
}

impl ControlEvent {
    /// Получить нормализованное значение (если применимо)
    /// Получить нормализованное значение (0.0-1.0), если применимо.
    pub fn normalized_value(&self) -> Option<f32> {
        match self {
    /// Событие поворотной ручки (энкодер).
            ControlEvent::Knob { normalized, .. } => Some(*normalized),
    /// Событие линейного фейдера.
            ControlEvent::Fader { normalized, .. } => Some(*normalized),
    /// MIDI Control Change сообщение.
    /// MIDI бэкенд.
            ControlEvent::MidiControl { normalized, .. } => Some(*normalized),
    /// Событие кнопки (нажата/отпущена).
            ControlEvent::Button { pressed, .. } => Some(if *pressed { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    
    /// Получить ID (если применимо)
    /// Получить ID элемента управления, если применимо.
    pub fn id(&self) -> Option<u32> {
        match self {
    /// Событие кнопки (нажата/отпущена).
            ControlEvent::Button { id, .. } => Some(*id),
    /// Событие поворотной ручки (энкодер).
            ControlEvent::Knob { id, .. } => Some(*id),
    /// Событие линейного фейдера.
            ControlEvent::Fader { id, .. } => Some(*id),
            _ => None,
        }
    }
}

/// Тип бэкенда
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
    /// Тип бэкенда.
pub enum BackendType {
    /// MIDI бэкенд.
    Midi,
    /// HID бэкенд.
    Hid,
    /// OSC бэкенд.
    Osc,
    /// Mackie Control Universal бэкенд.
    Mackie,
    /// Пользовательское событие.
    /// Пользовательский бэкенд.
    Custom,
}

impl BackendType {
    /// Получить имя бэкенда.
    pub fn name(&self) -> &'static str {
        match self {
    /// MIDI бэкенд.
            BackendType::Midi => "MIDI",
    /// HID бэкенд.
            BackendType::Hid => "HID",
    /// OSC бэкенд.
            BackendType::Osc => "OSC",
    /// Mackie Control Universal бэкенд.
            BackendType::Mackie => "Mackie",
    /// Пользовательское событие.
    /// Пользовательский бэкенд.
            BackendType::Custom => "Custom",
        }
    }
}

/// Информация об устройстве
#[derive(Debug, Clone)]
    /// Информация об устройстве.
    ///
    /// Содержит имя, тип бэкенда, список портов.
pub struct DeviceInfo {
    pub name: String,
    pub backend: BackendType,
    pub is_default: bool,
    pub input_ports: Vec<String>,
    pub output_ports: Vec<String>,
}

/// Базовый трейт для бэкендов контроллеров
pub trait ControlBackend: Send + Sync {
    /// Получить имя бэкенда
    /// Получить имя бэкенда.
    fn name(&self) -> &'static str;
    
    /// Получить тип бэкенда
    /// Получить тип бэкенда.
    fn backend_type(&self) -> BackendType;
    
    /// Инициализировать бэкенд
    /// Инициализировать бэкенд.
    fn init(&mut self) -> ControlResult<()>;
    
    /// Запустить получение событий
    /// Запустить получение событий.
    fn start(&mut self) -> ControlResult<()>;
    
    /// Остановить
    /// Остановить получение событий.
    fn stop(&mut self) -> ControlResult<()>;
    
    /// Подписаться на поток событий
    /// Подписаться на поток событий.
    fn subscribe(&self) -> broadcast::Receiver<ControlEvent>;
    
    /// Получить список доступных устройств
    /// Получить список доступных устройств.
    fn list_devices(&self) -> Vec<DeviceInfo>;
    
    /// Доступен ли бэкенд на этой платформе
    /// Проверить доступность бэкенда на платформе.
    fn is_available(&self) -> bool;
}