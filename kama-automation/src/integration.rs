use crate::{AutomationManager, SignalSender};
use kama_core_traits::{
    node::AudioNode,
    param::ParamValue,
    AudioError,
};

use kama_signal::{SignalDispatcher, AdvancedSignalDispatcher},

/// Адаптер для отправки сигналов в kama-core систему
pub struct KamaSignalSender {
    dispatcher: std::sync::Arc<std::sync::Mutex<Option<SignalDispatcher>>>,
}

impl SignalSender for KamaSignalSender {
    fn send_parameter_changed(&self, node_id: &str, param_id: &str, value: f32) {
        if let Some(dispatcher) = self.dispatcher.lock().unwrap().as_ref() {
            // Создаем и отправляем сигнал ParameterChanged
            // (нужна реализация совместимого типа сигнала)
        }
    }
}

impl KamaSignalSender {
    pub fn new() -> Self {
        Self {
            dispatcher: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }
    
    pub fn connect(&self, dispatcher: SignalDispatcher) {
        *self.dispatcher.lock().unwrap() = Some(dispatcher);
    }
}

/// Строитель для создания автоматизированных узлов
pub struct AutomationBuilder {
    manager: AutomationManager,
    signal_sender: std::sync::Arc<KamaSignalSender>,
}

impl AutomationBuilder {
    pub fn new(sample_rate: f64) -> Self {
        let signal_sender = std::sync::Arc::new(KamaSignalSender::new());
        
        Self {
            manager: AutomationManager::new(sample_rate)
                .with_signal_sender(signal_sender.clone()),
            signal_sender,
        }
    }
    
    pub fn create_automated_node<N: AudioNode + 'static>(
        &self,
        node: N,
        sample_rate: f32,
    ) -> crate::AutomatedNode<N> {
        let mut automated_node = crate::AutomatedNode::new(node, sample_rate);
        
        // Здесь можно настроить автоматизацию по умолчанию
        // или предоставить API для конфигурации
        
        automated_node
    }
    
    pub fn get_signal_sender(&self) -> std::sync::Arc<KamaSignalSender> {
        self.signal_sender.clone()
    }
}

/// Реестр типов автоматов для GUI и сериализации
pub struct AutomationRegistry {
    automaton_types: std::collections::HashMap<String, AutomatonType>,
}

#[derive(Debug, Clone)]
pub struct AutomatonType {
    pub name: String,
    pub description: String,
    pub default_params: Vec<AutomatonParam>,
    pub create_fn: Box<dyn Fn() -> Box<dyn crate::Automaton<Time = f64, Context = crate::AutomationContext>>>,
}

#[derive(Debug, Clone)]
pub struct AutomatonParam {
    pub name: String,
    pub default_value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
}

impl AutomationRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            automaton_types: std::collections::HashMap::new(),
        };
        
        // Регистрируем стандартные автоматы
        registry.register_lfo();
        // registry.register_clock();
        // registry.register_envelope();
        
        registry
    }
    
    fn register_lfo(&mut self) {
        self.automaton_types.insert("lfo".to_string(), AutomatonType {
            name: "LFO".to_string(),
            description: "Low Frequency Oscillator".to_string(),
            default_params: vec![
                AutomatonParam {
                    name: "frequency".to_string(),
                    default_value: 1.0,
                    min: Some(0.01),
                    max: Some(20.0),
                    step: Some(0.01),
                },
                AutomatonParam {
                    name: "amplitude".to_string(),
                    default_value: 0.5,
                    min: Some(0.0),
                    max: Some(1.0),
                    step: Some(0.01),
                },
            ],
            create_fn: Box::new(|| {
                Box::new(crate::LfoAutomaton {
                    id: 0,
                    frequency: 1.0,
                    amplitude: 0.5,
                    offset: 0.0,
                    waveform: crate::Waveform::Sine,
                    sync_to_clock: false,
                })
            }),
        });
    }
    
    pub fn create_automaton(&self, type_name: &str) -> Option<Box<dyn crate::Automaton<Time = f64, Context = crate::AutomationContext>>> {
        self.automaton_types.get(type_name).map(|t| (t.create_fn)())
    }
}