use serde::{Serialize, Deserialize};
use super::Signal;  // Теперь Signal определен в родительском модуле

/// Базовые сигналы для аудиообработки
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterChanged {
    pub node_id: String,
    pub parameter_id: String,
    pub value: f32,
    pub normalized_value: f32,
    pub timestamp: u64,
    pub source: SignalSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalSource {
    UserInterface,
    Automation,
    Midi { channel: u8, controller: u8 },
    Osc { address: String },
    Script,
    External,
}

impl Signal for ParameterChanged {}
impl Signal for SignalSource {}