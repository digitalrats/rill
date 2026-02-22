//! # Менеджер автоматизации — центральный координатор
//! 
//! Менеджер управляет коллекцией сервоприводов, синхронизирует их по времени
//! и обеспечивает доставку сигналов об изменениях.
//! 
//! ## Как это работает
//! 
//! 1. Создаётся менеджер с источником времени ([`TimeProvider`])
//! 2. Регистрируются сервоприводы через [`add_servo`] или удобные методы [`add_lfo`]
//! 3. В аудиопотоке регулярно вызывается [`update`], передавая количество обработанных семплов
//! 4. Менеджер обновляет все сервоприводы, и они отправляют сигналы в аудиограф
//! 
//! ## Потокобезопасность
//! 
//! Менеджер спроектирован для работы в однопоточном аудиопотоке.
//! Все методы должны вызываться из одного потока (обычно это аудиопоток).
//! Однако сервоприводы внутри менеджера требуют `Send + Sync`, так как
//! они могут быть переданы в другие потоки для инициализации.

// kama-automation/src/manager.rs
//! Менеджер автоматизации

use std::collections::HashMap;
use std::sync::Arc;
use kama_core_traits::time::{Clock, TimeProvider, SystemClock}; 
use kama_oscillators::control::LfoWaveform;
use crate::automaton::{LfoAutomaton, LfoWithEnvelopeAutomaton};
use crate::context::AutomationContext;
use crate::servo::{Servo, AnyServo, ParameterMapping};
use crate::signal::SignalSender;

/// Менеджер автоматизации с обобщённым типом часов
pub struct AutomationManager<C: Clock> {
    pub(crate) servos: HashMap<String, Box<dyn AnyServo>>,
    pub(crate) context: AutomationContext,
    pub(crate) clock: C,
    time_provider: Arc<dyn TimeProvider>,
}

impl<C: Clock> AutomationManager<C> {
    /// Создать новый менеджер автоматизации.
    ///
    /// # Аргументы
    /// * `time_provider` — источник времени
    /// * `clock` — часы для отсчёта семплов
    pub fn new(time_provider: Arc<dyn TimeProvider>, clock: C) -> Self {
        Self {
            servos: HashMap::new(),
            context: AutomationContext::new(time_provider.clone()),
            clock,
            time_provider,
        }
    }
    
    /// Добавить отправитель сигналов для всех будущих сервоприводов.
    pub fn with_signal_sender(mut self, sender: Arc<dyn SignalSender>) -> Self {
        self.context = self.context.with_signal_sender(sender);
        self
    }
    
    /// Добавить LFO для автоматизации параметра
    /// Добавить LFO для автоматизации параметра (удобная обёртка).
    pub fn add_lfo(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        target_node: &str,
        target_parameter: &str,
    ) {
        let automaton = Arc::new(
            LfoAutomaton::lfo(frequency, amplitude, offset, target_node, target_parameter)
        );
        
        // Создаём контекст с тем же signal_sender
        let mut context = AutomationContext::new(self.time_provider.clone());
        if let Some(sender) = &self.context.signal_sender {
            context = context.with_signal_sender(sender.clone());
        }
        
        let servo = Servo::new(
            id.to_string(),
            automaton,
            target_node.to_string(),
            target_parameter.to_string(),
            ParameterMapping::Linear,
            context,
        );
        
        self.add_servo(servo);
    }
    
    /// Добавить LFO с указанной формой волны
    /// Добавить LFO с указанной формой волны.
    pub fn add_lfo_with_waveform(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        target_node: &str,
        target_parameter: &str,
    ) {
        let automaton = Arc::new(
            LfoAutomaton::lfo_with_waveform(
                frequency, amplitude, offset, waveform,
                target_node, target_parameter
            )
        );
        
        let mut context = AutomationContext::new(self.time_provider.clone());
        if let Some(sender) = &self.context.signal_sender {
            context = context.with_signal_sender(sender.clone());
        }
        
        let servo = Servo::new(
            id.to_string(),
            automaton,
            target_node.to_string(),
            target_parameter.to_string(),
            ParameterMapping::Linear,
            context,
        );
        
        self.add_servo(servo);
    }
    
    /// Добавить LFO с огибающей
    /// Добавить LFO с огибающей.
    pub fn add_lfo_with_envelope(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        attack: f64,
        release: f64,
        target_node: &str,
        target_parameter: &str,
    ) {
        let automaton = Arc::new(
            LfoWithEnvelopeAutomaton::lfo_with_envelope(
                frequency, amplitude, offset, attack, release,
                target_node, target_parameter
            )
        );
        
        let mut context = AutomationContext::new(self.time_provider.clone());
        if let Some(sender) = &self.context.signal_sender {
            context = context.with_signal_sender(sender.clone());
        }
        
        let servo = Servo::new(
            id.to_string(),
            automaton,
            target_node.to_string(),
            target_parameter.to_string(),
            ParameterMapping::Linear,
            context,
        );
        
        self.add_servo(servo);
    }
    
    pub fn add_servo<A>(&mut self, mut servo: Servo<A>) 
    where
        A: crate::automaton::Automaton<Time = f64, Context = AutomationContext> + 'static,
        A::Action: Default + 'static,
    {
        // Создаём новый контекст с правильным signal_sender
        let mut context = AutomationContext::new(self.time_provider.clone());
        if let Some(sender) = &self.context.signal_sender {
            context = context.with_signal_sender(sender.clone());
        }
        servo.context = context;
        
        self.servos.insert(servo.id.clone(), Box::new(servo));
    }
    
    /// Обновить все сервоприводы.
    /// Должен вызываться из аудиопотока после обработки каждого блока.
    pub fn update(&mut self, sample_count: usize) {
        let samples = sample_count as u64;
        let new_position = self.clock.advance(samples);
        
        // Вычисляем текущее время в секундах
        let current_time = new_position as f64 / self.clock.sample_rate();
        
        for servo in self.servos.values_mut() {
            let _ = servo.update(current_time);
        }
    }
    
    pub fn set_signal_sender(&mut self, sender: Arc<dyn SignalSender>) {
        self.context.set_signal_sender(sender);
    }
    
    /// Получить ссылку на сервопривод по ID (для чтения).
    pub fn get_servo(&self, id: &str) -> Option<&dyn AnyServo> {
        self.servos.get(id).map(|b| b.as_ref())
    }
    
    /// Получить мутабельную ссылку на сервопривод по ID.
    pub fn get_servo_mut(&mut self, id: &str) -> Option<&mut Box<dyn AnyServo>> {
        self.servos.get_mut(id)
    }
    
    /// Удалить сервопривод. Возвращает `true`, если сервопривод существовал.
    pub fn remove_servo(&mut self, id: &str) -> bool {
        self.servos.remove(id).is_some()
    }
    
    /// Остановить все сервоприводы и сбросить время.
    pub fn clear(&mut self) {
        self.servos.clear();
        self.clock.reset();
    }
    
    pub fn servos(&self) -> &HashMap<String, Box<dyn AnyServo>> {
        &self.servos
    }
    
    pub fn context(&self) -> &AutomationContext {
        &self.context
    }
}

// Type alias для удобства использования с SystemClock
pub type DefaultAutomationManager = AutomationManager<SystemClock>;

impl DefaultAutomationManager {
    pub fn new_default(time_provider: Arc<dyn TimeProvider>) -> Self {
        let clock = SystemClock::new(
            time_provider.sample_rate(), 
            120.0
        );
        AutomationManager::new(time_provider, clock)
    }
}