//! Абстракции управления без конкретных реализаций

/// Хост параметров для управления
pub trait ParameterHost: Send + Sync {
    fn get_parameter(&self, id: &str) -> Option<f32>;
    fn set_parameter(&mut self, id: &str, value: f32);
    fn get_parameter_info(&self, id: &str) -> Option<ParameterInfo>;
    fn list_parameters(&self) -> Vec<String>;
}

/// Информация о параметре
#[derive(Debug, Clone)]
pub struct ParameterInfo {
    pub id: String,
    pub name: String,
    pub range: (f32, f32),
    pub default: f32,
    pub step: Option<f32>,
    pub unit: Option<String>,
}

/// Протокол управления
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlProtocol {
    Midi,
    Osc,
    Mackie,
    Http,
    WebSocket,
    Custom,
}

/// Конфигурация управления
pub struct ControlConfig {
    pub enabled_protocols: Vec<ControlProtocol>,
    pub parameter_host: Box<dyn ParameterHost>,
}

impl ControlConfig {
    pub fn new(host: Box<dyn ParameterHost>) -> Self {
        Self {
            enabled_protocols: Vec::new(),
            parameter_host: host,
        }
    }
    
    pub fn with_protocol(mut self, protocol: ControlProtocol) -> Self {
        self.enabled_protocols.push(protocol);
        self
    }
}
