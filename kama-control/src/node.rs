use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use kama_core_traits::{
    AudioNode, 
    NodeMetadata, 
    NodeCategory, 
    AudioError, 
    NodeTypeId,
    NodeId,
    param::{ParamValue, ParamType, ParamMetadata},
};

use kama_signal::{ParameterChanged, SignalSource};

#[cfg(feature = "automation")]
use kama_automation::{Servo, AutomationContext, Automaton};

use crate::backend::ControlEvent;
use crate::mapping::{Mapping, EventPattern, Target, Transform};
use crate::error::ControlResult;

/// Узел управления для AudioGraph
pub struct ControlNode {
    /// Получатель событий
    event_rx: broadcast::Receiver<ControlEvent>,
    /// Маппинги событий на параметры
    mappings: Vec<Mapping>,
    /// Кэш последних значений для сглаживания
    last_values: HashMap<String, f32>,
    
    #[cfg(feature = "automation")]
    /// Сервы для плавного изменения
    servos: HashMap<String, Servo<Box<dyn Automaton<Time = f64, Context = AutomationContext>>>>,
    
    #[cfg(feature = "automation")]
    /// Контекст автоматизации
    context: AutomationContext,
    
    /// Отправитель сигналов (для ParameterChanged)
    signal_tx: Option<Arc<RwLock<Box<dyn Fn(ParameterChanged) + Send + Sync>>>>,
    
    /// Имя узла
    name: String,
    
    /// Количество обработанных событий
    event_count: usize,
}

impl ControlNode {
    /// Создать новый узел управления
    pub fn new(event_rx: broadcast::Receiver<ControlEvent>) -> Self {
        Self {
            event_rx,
            mappings: Vec::new(),
            last_values: HashMap::new(),
            
            #[cfg(feature = "automation")]
            servos: HashMap::new(),
            
            #[cfg(feature = "automation")]
            context: AutomationContext::new(Arc::new(DummyTimeProvider)), // нужно будет заменить
            
            signal_tx: None,
            name: "ControlNode".to_string(),
            event_count: 0,
        }
    }
    
    /// Добавить маппинг
    pub fn add_mapping(&mut self, mapping: Mapping) {
        self.mappings.push(mapping);
    }
    
    /// Добавить маппинг из строк (удобно для скриптов)
    pub fn add_mapping_str(
        &mut self,
        pattern: &str,
        target_node: NodeId,
        target_param: &str,
        min: f32,
        max: f32,
        transform: Transform,
    ) -> ControlResult<()> {
        let pattern = match pattern {
            p if p.starts_with("button:") => {
                let id = p[7..].parse().map_err(|_| crate::error::ControlError::Mapping("Invalid button ID".into()))?;
                EventPattern::ButtonId(id)
            }
            p if p.starts_with("knob:") => {
                let id = p[5..].parse().map_err(|_| crate::error::ControlError::Mapping("Invalid knob ID".into()))?;
                EventPattern::KnobId(id)
            }
            p if p.starts_with("fader:") => {
                let id = p[6..].parse().map_err(|_| crate::error::ControlError::Mapping("Invalid fader ID".into()))?;
                EventPattern::FaderId(id)
            }
            p if p.starts_with("midi:") => {
                let parts: Vec<&str> = p[5..].split(':').collect();
                if parts.len() == 2 {
                    let channel = parts[0].parse().ok();
                    let controller = parts[1].parse().map_err(|_| crate::error::ControlError::Mapping("Invalid controller".into()))?;
                    EventPattern::MidiControl { channel, controller }
                } else {
                    EventPattern::AnyMidi
                }
            }
            p if p.starts_with("osc:") => EventPattern::OscAddress(p[4..].to_string()),
            _ => return Err(crate::error::ControlError::Mapping(format!("Unknown pattern: {}", pattern))),
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
    
    /// Установить отправитель сигналов
    pub fn set_signal_sender<F>(&mut self, sender: F)
    where
        F: Fn(ParameterChanged) + Send + Sync + 'static,
    {
        self.signal_tx = Some(Arc::new(RwLock::new(Box::new(sender))));
    }
    
    /// Установить имя узла
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
    }
    
    /// Получить статистику
    pub fn stats(&self) -> (usize, usize) {
        (self.event_count, self.mappings.len())
    }
    
    #[cfg(feature = "automation")]
    /// Установить контекст автоматизации
    pub fn set_automation_context(&mut self, context: AutomationContext) {
        self.context = context;
    }
}

// Заглушка для TimeProvider
#[derive(Debug)]
struct DummyTimeProvider;

impl kama_core_traits::Clock for DummyTimeProvider {
    fn sample_rate(&self) -> f64 { 44100.0 }
    fn position_samples(&self) -> u64 { 0 }
    fn advance(&self, _samples: u64) -> u64 { 0 }
    fn reset(&self) {}
}

impl kama_core_traits::TimeProvider for DummyTimeProvider {
    fn bpm(&self) -> f64 { 120.0 }
    fn set_bpm(&self, _bpm: f64) {}
    fn tick_info(&self) -> kama_core_traits::TickInfo {
        kama_core_traits::TickInfo {
            bar: 0,
            beat: 0,
            sixteenth: 0,
            sample_pos: 0,
        }
    }
}

impl AudioNode for ControlNode {
    fn process(&mut self, _inputs: &[&[f32]], _outputs: &mut [&mut [f32]]) -> Result<(), AudioError> {
        // Проверяем новые события (без блокировки)
        while let Ok(event) = self.event_rx.try_recv() {
            self.event_count += 1;
            
            // Применяем все подходящие маппинги
            for mapping in &self.mappings {
                if let Some(value) = mapping.apply(&event) {
                    let key = format!("{:?}:{}", mapping.target.node_id, mapping.target.param_name);
                    
                    // Проверяем, изменилось ли значение (с допуском)
                    let last = self.last_values.get(&key).copied().unwrap_or(f32::NAN);
                    if last.is_nan() || (value - last).abs() > 0.001 {
                        self.last_values.insert(key.clone(), value);
                        
                        // Отправляем сигнал, если есть отправитель
                        if let Some(tx) = &self.signal_tx {
                            let signal = ParameterChanged {
                                node_id: format!("{:?}", mapping.target.node_id),
                                parameter_id: mapping.target.param_name.clone(),
                                value,
                                normalized_value: event.normalized_value().unwrap_or(0.0),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap()
                                    .as_millis() as u64,
                                source: kama_signal::SignalSource::Midi { 
                                    channel: 0, 
                                    controller: 0 
                                },
                            };
                            
                            if let Some(lock) = tx.try_read() {
                                lock(signal);
                            }
                        }
                    }
                }
            }
        }
        
        Ok(())
    }
    
    fn get_param(&self, name: &str) -> Option<ParamValue> {
        match name {
            "event_count" => Some(ParamValue::Int(self.event_count as i32)),
            "mapping_count" => Some(ParamValue::Int(self.mappings.len() as i32)),
            _ => None,
        }
    }
    
    fn set_param(&mut self, name: &str, value: ParamValue) -> Result<(), AudioError> {
        match (name, value) {
            ("clear_mappings", ParamValue::Bool(true)) => {
                self.mappings.clear();
                self.last_values.clear();
                Ok(())
            }
            _ => Err(AudioError::Parameter(format!("Unknown parameter: {}", name))),
        }
    }
    
    fn init(&mut self, _sample_rate: f32) {
        // Ничего не делаем
    }
    
    fn reset(&mut self) {
        self.last_values.clear();
        self.event_count = 0;
    }
    
    fn num_inputs(&self) -> usize { 0 }
    fn num_outputs(&self) -> usize { 0 }
    
    fn node_type_id(&self) -> NodeTypeId {
        NodeTypeId::of::<Self>()
    }
    
    fn metadata(&self) -> NodeMetadata {
        NodeMetadata {
            name: self.name.clone(),
            category: NodeCategory::Utility,
            description: "Control node that maps controller events to parameters".to_string(),
            author: "Kama Control".to_string(),
            version: "0.1.0".to_string(),
            parameters: vec![
                ParamMetadata {
                    name: "event_count".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(0),
                    min: Some(0.0),
                    max: None,
                    step: Some(1.0),
                    unit: Some("events".to_string()),
                    choices: None,
                },
                ParamMetadata {
                    name: "mapping_count".to_string(),
                    typ: ParamType::Int,
                    default: ParamValue::Int(0),
                    min: Some(0.0),
                    max: None,
                    step: Some(1.0),
                    unit: Some("mappings".to_string()),
                    choices: None,
                },
            ],
        }
    }
}