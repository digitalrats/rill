use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::error::ControlResult;

/// Событие контроллера
#[derive(Debug, Clone, PartialEq)]
pub enum ControlEvent {
    /// Кнопка
    Button {
        id: u32,
        pressed: bool,
    },
    
    /// Поворотная ручка (энкодер)
    Knob {
        id: u32,
        value: f32,           // 0.0 - 1.0
        normalized: f32,       // то же, для совместимости
    },
    
    /// Фейдер (линейный ползунок)
    Fader {
        id: u32,
        value: f32,           // 0.0 - 1.0
        normalized: f32,
    },
    
    /// MIDI сообщение
    Midi {
        channel: u8,          // 0-15
        message: Vec<u8>,
    },
    
    /// MIDI Control Change (специализированное)
    MidiControl {
        channel: u8,
        controller: u8,
        value: u8,            // 0-127
        normalized: f32,      // 0.0 - 1.0
    },
    
    /// MIDI Note (для клавиатур)
    MidiNote {
        channel: u8,
        note: u8,
        velocity: u8,
        on: bool,
    },
    
    /// OSC сообщение
    Osc {
        address: String,
        args: Vec<f32>,
    },
    
    /// Пользовательское событие
    Custom(String, Vec<f32>),
}

impl ControlEvent {
    /// Получить нормализованное значение (если применимо)
    pub fn normalized_value(&self) -> Option<f32> {
        match self {
            ControlEvent::Knob { normalized, .. } => Some(*normalized),
            ControlEvent::Fader { normalized, .. } => Some(*normalized),
            ControlEvent::MidiControl { normalized, .. } => Some(*normalized),
            ControlEvent::Button { pressed, .. } => Some(if *pressed { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    
    /// Получить ID (если применимо)
    pub fn id(&self) -> Option<u32> {
        match self {
            ControlEvent::Button { id, .. } => Some(*id),
            ControlEvent::Knob { id, .. } => Some(*id),
            ControlEvent::Fader { id, .. } => Some(*id),
            _ => None,
        }
    }
}

/// Тип бэкенда
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    Midi,
    Hid,
    Osc,
    Mackie,
    Custom,
}

impl BackendType {
    pub fn name(&self) -> &'static str {
        match self {
            BackendType::Midi => "MIDI",
            BackendType::Hid => "HID",
            BackendType::Osc => "OSC",
            BackendType::Mackie => "Mackie",
            BackendType::Custom => "Custom",
        }
    }
}

/// Информация об устройстве
#[derive(Debug, Clone)]
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
    fn name(&self) -> &'static str;
    
    /// Получить тип бэкенда
    fn backend_type(&self) -> BackendType;
    
    /// Инициализировать бэкенд
    fn init(&mut self) -> ControlResult<()>;
    
    /// Запустить получение событий
    fn start(&mut self) -> ControlResult<()>;
    
    /// Остановить
    fn stop(&mut self) -> ControlResult<()>;
    
    /// Подписаться на поток событий
    fn subscribe(&self) -> broadcast::Receiver<ControlEvent>;
    
    /// Получить список доступных устройств
    fn list_devices(&self) -> Vec<DeviceInfo>;
    
    /// Доступен ли бэкенд на этой платформе
    fn is_available(&self) -> bool;
}