use std::sync::Arc;
use parking_lot::RwLock;
use thiserror::Error;

// Re-export типов из kama-core для удобства
pub use kama_core::param::{ParamValue, ParamType};
pub use kama_core::signal::{Signal, SignalHandler};

// --- Типы ошибок ---
#[derive(Error, Debug)]
pub enum AutomationError {
    #[error("Automaton error: {0}")]
    Automaton(String),
    
    #[error("Parameter error: {0}")]
    Parameter(String),
    
    #[error("Servo error: {0}")]
    Servo(String),
    
    #[error("Clock error: {0}")]
    Clock(String),
    
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

pub type AutomationResult<T> = Result<T, AutomationError>;

// --- Добавить недостающие типы ---

#[derive(Debug, Clone, Default)]
pub struct EnvelopeState {
    pub stage: EnvelopeStage,
    pub value: f64,
    pub samples_elapsed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

impl Default for EnvelopeStage {
    fn default() -> Self {
        EnvelopeStage::Off
    }
}

#[derive(Debug, Clone, Default)]
pub struct LfoState {
    pub phase: f64,
    pub envelope_state: Option<EnvelopeState>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum LfoAction {
    #[default]
    None,
    SetFrequency(f64),
    SetAmplitude(f64),
    Trigger,
}

// --- Контекст выполнения автоматов, совместимый с kama-core ---

#[derive(Debug, Clone)]
pub struct AutomationContext {
    pub sample_rate: f64,
    pub global_time: f64,
    pub parameters: Arc<ParameterMap>,
    pub signal_sender: Option<Arc<dyn SignalSender>>,
}

impl AutomationContext {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            global_time: 0.0,
            parameters: Arc::new(ParameterMap::new()),
            signal_sender: None,
        }
    }
    
    pub fn with_signal_sender(mut self, sender: Arc<dyn SignalSender>) -> Self {
        self.signal_sender = Some(sender);
        self
    }
}

// --- Интерфейс для отправки сигналов kama-core ---

pub trait SignalSender: std::fmt::Debug + Send + Sync {
    fn send_parameter_changed(&self, node_id: &str, param_id: &str, value: f32);
}

// --- Карта параметров с поддержкой kama-core ParamValue ---

#[derive(Debug, Clone)]
pub struct ParameterData {
    pub value: f64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub unit: Option<String>,
}

#[derive(Debug, Default)]
pub struct ParameterMap {
    params: RwLock<std::collections::HashMap<String, ParameterData>>,
}

impl ParameterMap {
    pub fn new() -> Self {
        Self {
            params: RwLock::new(std::collections::HashMap::new()),
        }
    }
    
    pub fn set_parameter(&self, name: &str, value: f64) {
        let mut params = self.params.write();
        if let Some(data) = params.get_mut(name) {
            data.value = value;
        } else {
            params.insert(name.to_string(), ParameterData {
                value,
                min: None,
                max: None,
                step: None,
                unit: None,
            });
        }
    }
    
    pub fn get_parameter(&self, name: &str) -> Option<f64> {
        let params = self.params.read();
        params.get(name).map(|data| data.value)
    }
}

// --- Алгебраические типы для автоматов ---

/// Автомат: (time, context, action, state) -> new_state
pub trait Automaton: Send + Sync {
    type Time;
    type Context;
    type Action: Clone + Default + Send + Sync + 'static;
    type State: Clone + Send + Sync + 'static;
    
    /// Основная функция автомата
    fn step(
        &self,
        time: Self::Time,
        context: &Self::Context,
        action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<Self::Action>);
    
    /// Начальное состояние
    fn initial_state(&self) -> Self::State;
    
    /// Имя автомата для отладки
    fn name(&self) -> &str;
    
    /// Извлечение значения из состояния (для сервоприводов)
    fn extract_value(&self, state: &Self::State) -> f64;
}

// --- LFO автомат ---

pub struct LfoAutomaton {
    frequency: f64,
    amplitude: f64,
    offset: f64,
    attack_time: f64,
    release_time: f64,
}

impl LfoAutomaton {
    pub fn new(frequency: f64, amplitude: f64, offset: f64) -> Self {
        Self {
            frequency,
            amplitude,
            offset,
            attack_time: 0.01,
            release_time: 0.01,
        }
    }
    
    pub fn with_envelope(mut self, attack: f64, release: f64) -> Self {
        self.attack_time = attack;
        self.release_time = release;
        self
    }
}

impl Automaton for LfoAutomaton {
    type Time = f64;
    type Context = AutomationContext;
    type Action = LfoAction;
    type State = LfoState;
    
    fn step(
        &self,
        time: f64,
        context: &AutomationContext,
        action: LfoAction,
        state: &LfoState,
    ) -> (LfoState, Option<LfoAction>) {
        let mut new_state = state.clone();
        let next_action = None;
        
        // Обработка действий
        match action {
            LfoAction::SetFrequency(freq) => {
                // TODO: Обновить частоту
                // Пока игнорируем
                let _ = freq;
            }
            LfoAction::SetAmplitude(amp) => {
                // TODO: Обновить амплитуду
                let _ = amp;
            }
            LfoAction::Trigger => {
                // Запустить envelope
                new_state.envelope_state = Some(EnvelopeState {
                    stage: EnvelopeStage::Attack,
                    value: 0.0,
                    samples_elapsed: 0,
                });
            }
            LfoAction::None => {}
        }
        
        // Обновить фазу LFO
        let phase_increment = self.frequency / context.sample_rate;
        new_state.phase += phase_increment;
        if new_state.phase >= 1.0 {
            new_state.phase -= 1.0;
        }
        
        // Применить envelope если есть
        let envelope_gain = if let Some(ref mut envelope) = new_state.envelope_state {
            match envelope.stage {
                EnvelopeStage::Attack => {
                    let attack_samples = (self.attack_time * context.sample_rate) as usize;
                    if envelope.samples_elapsed < attack_samples {
                        envelope.value = envelope.samples_elapsed as f64 / attack_samples as f64;
                        envelope.samples_elapsed += 1;
                    } else {
                        envelope.stage = EnvelopeStage::Decay;
                        envelope.samples_elapsed = 0;
                        envelope.value = 1.0;
                    }
                    envelope.value
                }
                EnvelopeStage::Decay => {
                    envelope.stage = EnvelopeStage::Sustain;
                    envelope.value = 1.0;
                    envelope.value
                }
                EnvelopeStage::Sustain => {
                    envelope.value
                }
                EnvelopeStage::Release => {
                    let release_samples = (self.release_time * context.sample_rate) as usize;
                    if envelope.samples_elapsed < release_samples {
                        envelope.value = 1.0 - (envelope.samples_elapsed as f64 / release_samples as f64);
                        envelope.samples_elapsed += 1;
                    } else {
                        envelope.stage = EnvelopeStage::Off;
                        envelope.value = 0.0;
                    }
                    envelope.value
                }
                EnvelopeStage::Off => 0.0,
            }
        } else {
            1.0
        };
        
        // Вычислить выходное значение
        let _output_value = (new_state.phase * 2.0 * std::f64::consts::PI).sin() 
            * self.amplitude * envelope_gain 
            + self.offset;
        
        (new_state, next_action)
    }
    
    fn initial_state(&self) -> LfoState {
        LfoState {
            phase: 0.0,
            envelope_state: Some(EnvelopeState::default()),
        }
    }
    
    fn name(&self) -> &str {
        "LFO"
    }
    
    fn extract_value(&self, state: &LfoState) -> f64 {
        let envelope_gain = state.envelope_state.as_ref()
            .map(|e| e.value)
            .unwrap_or(1.0);
        
        (state.phase * 2.0 * std::f64::consts::PI).sin() 
            * self.amplitude * envelope_gain 
            + self.offset
    }
}

// --- Сервопривод (управление параметром) ---

pub enum ParameterMapping {
    Linear,
    Exponential,
    Logarithmic,
    Custom(Box<dyn Fn(f64) -> f64 + Send + Sync>),
}

pub struct Servo<A>
where
    A: Automaton<Time = f64, Context = AutomationContext>,
    A::Action: Default,
{
    pub id: String,
    pub automaton: Arc<A>,
    pub target_node: String,
    pub target_parameter: String,
    pub mapping: ParameterMapping,
    pub min_value: f64,
    pub max_value: f64,
    pub enabled: bool,
    
    // Внутреннее состояние
    state: A::State,
    last_value: f64,
    last_update_time: f64,
    
    // Контекст должен быть доступен, но не храниться в Arc
    context: AutomationContext,
}

impl<A> Servo<A>
where
    A: Automaton<Time = f64, Context = AutomationContext>,
    A::Action: Default,
{
    pub fn new(
        id: String,
        automaton: Arc<A>,
        target_node: String,
        target_parameter: String,
        mapping: ParameterMapping,
        context: AutomationContext,
    ) -> Self {
        Self {
            id,
            automaton: automaton.clone(),
            target_node,
            target_parameter,
            mapping,
            min_value: 0.0,
            max_value: 1.0,
            enabled: true,
            state: automaton.initial_state(),
            last_value: 0.0,
            last_update_time: 0.0,
            context,
        }
    }
    
    pub fn update(&mut self, time: f64) -> Option<f64> {
        if !self.enabled {
            return None;
        }
        
        // Вычислить дельту времени
        let delta_time = time - self.last_update_time;
        if delta_time <= 0.0 {
            return None;
        }
        
        // Выполнить шаг автомата
        let (new_state, _next_action) = self.automaton.step(
            time,
            &self.context,
            A::Action::default(),
            &self.state,
        );
        
        self.state = new_state;
        
        // Извлечь значение
        let raw_value = self.automaton.extract_value(&self.state);
        
        // Применить маппинг
        let mapped_value = match &self.mapping {
            ParameterMapping::Linear => raw_value,
            ParameterMapping::Exponential => raw_value.exp(),
            ParameterMapping::Logarithmic => raw_value.abs().ln_1p(),
            ParameterMapping::Custom(func) => func(raw_value),
        };
        
        // Ограничить диапазон
        let clamped_value = mapped_value.clamp(self.min_value, self.max_value);
        
        // Отправить сигнал если значение изменилось
        if (clamped_value - self.last_value).abs() > 1e-6 {
            if let Some(sender) = &self.context.signal_sender {
                sender.send_parameter_changed(
                    &self.target_node,
                    &self.target_parameter,
                    clamped_value as f32,
                );
            }
            self.last_value = clamped_value;
        }
        
        self.last_update_time = time;
        Some(clamped_value)
    }
    
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    pub fn set_range(&mut self, min: f64, max: f64) {
        self.min_value = min;
        self.max_value = max;
    }
}

// --- Менеджер автоматизации ---

pub struct AutomationManager {
    servos: std::collections::HashMap<String, Box<dyn AnyServo>>,
    context: AutomationContext,
    clock: Clock,
}

trait AnyServo: Send + Sync {
    fn update(&mut self, time: f64) -> Option<f64>;
    fn id(&self) -> &str;
    fn set_enabled(&mut self, enabled: bool);
}

impl<A> AnyServo for Servo<A>
where
    A: Automaton<Time = f64, Context = AutomationContext> + 'static,
    A::Action: Default + 'static,
{
    fn update(&mut self, time: f64) -> Option<f64> {
        Servo::update(self, time)
    }
    
    fn id(&self) -> &str {
        &self.id
    }
    
    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

impl AutomationManager {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            servos: std::collections::HashMap::new(),
            context: AutomationContext::new(sample_rate),
            clock: Clock::new(sample_rate),
        }
    }
    
    pub fn add_servo<A>(&mut self, mut servo: Servo<A>) 
    where
        A: Automaton<Time = f64, Context = AutomationContext> + 'static,
        A::Action: Default + 'static,
    {
        // Обновить контекст сервопривода
        servo.context = self.context.clone();
        self.servos.insert(servo.id.clone(), Box::new(servo));
    }
    
    pub fn update(&mut self, sample_count: usize) {
        // Обновить время
        let time = self.clock.advance(sample_count);
        
        // Обновить контекст
        let mut context = self.context.clone();
        context.global_time = time;
        self.context = context;
        
        // Обновить все сервоприводы
        for servo in self.servos.values_mut() {
            servo.update(time);
        }
    }
    
    pub fn set_signal_sender(&mut self, sender: Arc<dyn SignalSender>) {
        let mut context = self.context.clone();
        context.signal_sender = Some(sender);
        self.context = context;
    }
    
    pub fn add_lfo(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        target_node: &str,
        target_parameter: &str,
    ) {
        let automaton = Arc::new(LfoAutomaton::new(frequency, amplitude, offset));
        let servo = Servo::new(
            id.to_string(),
            automaton,
            target_node.to_string(),
            target_parameter.to_string(),
            ParameterMapping::Linear,
            self.context.clone(),
        );
        
        self.add_servo(servo);
    }
}

// --- Часы (тайминг) ---

pub struct Clock {
    sample_rate: f64,
    current_time: f64,
}

impl Clock {
    pub fn new(sample_rate: f64) -> Self {
        Self {
            sample_rate,
            current_time: 0.0,
        }
    }
    
    pub fn advance(&mut self, samples: usize) -> f64 {
        let delta = samples as f64 / self.sample_rate;
        self.current_time += delta;
        self.current_time
    }
    
    pub fn reset(&mut self) {
        self.current_time = 0.0;
    }
    
    pub fn current_time(&self) -> f64 {
        self.current_time
    }
}

// --- Пример реализации SignalSender для тестирования ---

#[derive(Debug)]
pub struct TestSignalSender {
    pub sent_signals: RwLock<Vec<(String, String, f32)>>,
}

impl TestSignalSender {
    pub fn new() -> Self {
        Self {
            sent_signals: RwLock::new(Vec::new()),
        }
    }

    pub fn clear_signals(&self) {
        let mut signals = self.sent_signals.write();
        signals.clear();
    }
    
    pub fn get_signals_count(&self) -> usize {
        let signals = self.sent_signals.read();
        signals.len()
    }
    
    pub fn get_signals_for_param(&self, node_id: &str, param_id: &str) -> Vec<f32> {
        let signals = self.sent_signals.read();
        signals.iter()
            .filter(|(n, p, _)| n == node_id && p == param_id)
            .map(|(_, _, v)| *v)
            .collect()
    }
}

impl SignalSender for TestSignalSender {
    fn send_parameter_changed(&self, node_id: &str, param_id: &str, value: f32) {
        let mut signals = self.sent_signals.write();
        signals.push((node_id.to_string(), param_id.to_string(), value));
    }
}

// --- Тесты ---

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_lfo_automaton() {
        let lfo = LfoAutomaton::new(1.0, 0.5, 0.0);
        let context = AutomationContext::new(44100.0);
        let state = lfo.initial_state();
        
        let (new_state, action) = lfo.step(
            0.1,
            &context,
            LfoAction::None,
            &state,
        );
        
        assert!(new_state.phase > 0.0);
        assert_eq!(action, None);
    }
    
    #[test]
    fn test_automation_manager() {
        let mut manager = AutomationManager::new(44100.0);
        
        // Добавить тестовый LFO
        manager.add_lfo(
            "test_lfo",
            1.0,
            0.5,
            0.0,
            "test_node",
            "gain",
        );
        
        // Обновить менеджер
        manager.update(512);
        
        // Проверить что время обновилось
        assert!(manager.clock.current_time() > 0.0);
    }
    
    #[test]
    fn test_parameter_map() {
        let map = ParameterMap::new();
        map.set_parameter("gain", 0.75);
        
        let value = map.get_parameter("gain");
        assert_eq!(value, Some(0.75));
    }
}