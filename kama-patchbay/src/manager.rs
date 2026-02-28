use crate::automaton::{Automaton, FunctionAutomaton, LfoAutomaton};
use crate::core::ParameterTarget;
use crate::queues::{AutomationCommand, CommandQueue, TelemetryData, TelemetryQueue};
use crate::sensor::{Sensor, SensorManager, SensorProcessor};
use crate::servo::{ParameterMapping, Servo};
use crossbeam_channel::Receiver;
use kama_core::time::{Clock, SystemClock, TimeProvider};
use kama_core::traits::{NodeId, ParameterId, PortId};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Статистика системы
#[derive(Debug, Default, Clone, Copy)]
pub struct AutomationStats {
    pub servos_active: usize,
    pub sensors_active: usize,
    pub commands_processed: u64,
    pub telemetry_received: u64,
}

/// Главный менеджер, объединяющий автоматизацию и управление
pub struct AutomationManager {
    /// Серво (управление параметрами)
    servos: HashMap<String, Box<dyn crate::servo::AnyServo>>,
    
    /// Сенсоры (обратная связь)
    sensors: HashMap<String, Box<dyn crate::sensor::AnySensor>>,
    
    /// Контекст выполнения
    context: crate::core::AutomationContext,
    
    /// Очередь команд (в audio-поток)
    command_queue: CommandQueue,
    
    /// Очередь телеметрии (из audio-потока)
    telemetry_queue: TelemetryQueue,
    
    /// Получатель телеметрии для control-потока
    telemetry_rx: Receiver<TelemetryData>,
    
    /// Статистика
    stats: AutomationStats,
    
    /// Поток обработки
    thread_handle: Option<thread::JoinHandle<()>>,
    
    /// Флаг работы
    running: bool,
}

impl AutomationManager {
    /// Создать новый менеджер
    pub fn new(time_provider: Arc<dyn TimeProvider>, clock: impl Clock + 'static) -> Self {
        let telemetry_queue = TelemetryQueue::new();
        let telemetry_rx = telemetry_queue.receiver();
        
        Self {
            servos: HashMap::new(),
            sensors: HashMap::new(),
            context: crate::core::AutomationContext::new(time_provider),
            command_queue: CommandQueue::new(),
            telemetry_queue,
            telemetry_rx,
            stats: AutomationStats::default(),
            thread_handle: None,
            running: false,
        }
    }
    
    /// Запустить поток обработки
    pub fn start(&mut self) {
        if self.running {
            return;
        }
        
        self.running = true;
        let running = self.running;
        let telemetry_rx = self.telemetry_rx.clone();
        
        let handle = thread::spawn(move || {
            Self::run_control_loop(running, telemetry_rx);
        });
        
        self.thread_handle = Some(handle);
    }
    
    /// Остановить поток обработки
    pub fn stop(&mut self) {
        self.running = false;
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }
    }
    
    /// Цикл обработки в control-потоке
    fn run_control_loop(running: bool, telemetry_rx: Receiver<TelemetryData>) {
        while running {
            // Обрабатываем входящую телеметрию
            while let Ok(data) = telemetry_rx.try_recv() {
                match data {
                    TelemetryData::SensorValue { sensor_id, value, timestamp } => {
                        // Здесь можно обновлять GUI, логировать, 
                        // или использовать для адаптивной автоматизации
                        log::debug!("Sensor {}: {}", sensor_id, value);
                    }
                    TelemetryData::ParameterValue { port, parameter, value, timestamp } => {
                        log::debug!("Parameter {} = {}", parameter, value);
                    }
                    _ => {}
                }
            }
            
            thread::sleep(Duration::from_millis(1));
        }
    }
    
    /// Добавить серво
    pub fn add_servo<A>(&mut self, servo: Servo<A>) 
    where
        A: Automaton<Time = f64, Context = crate::core::AutomationContext> + 'static,
        A::Action: Default + 'static,
    {
        let id = servo.id.clone();
        self.servos.insert(id, Box::new(servo));
        self.stats.servos_active = self.servos.len();
    }
    
    /// Добавить сенсор
    pub fn add_sensor(&mut self, sensor: Sensor) {
        let id = sensor.id().to_string();
        self.sensors.insert(id, Box::new(sensor));
        self.stats.sensors_active = self.sensors.len();
    }
    
    /// Создать LFO серво
    pub fn create_lfo(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        target: ParameterTarget,
    ) {
        use crate::automaton::LfoAutomaton;
        
        let automaton = Arc::new(LfoAutomaton::lfo(
            frequency,
            amplitude,
            offset,
            target.parameter.clone(),
        ));
        
        let servo = Servo::new(
            id.to_string(),
            automaton,
            target,
            ParameterMapping::Linear,
            self.context.clone(),
        );
        
        self.add_servo(servo);
    }
    
    /// Создать сенсор для RMS
    pub fn create_rms_sensor(
        &mut self,
        id: &str,
        source: crate::core::SensorSource,
        window_size: usize,
    ) {
        use crate::sensor::{RmsProcessor, Sensor};
        
        let processor = Arc::new(RmsProcessor::new(window_size));
        let sensor = Sensor::new(
            id,
            source,
            processor,
            self.telemetry_queue.sender(),
        );
        
        self.add_sensor(sensor);
    }
    
    /// Обновить состояние (вызывается из audio-потока)
    pub fn update(&mut self, sample_count: usize, sample_rate: f32) {
        // Обновляем время
        self.context.advance(sample_count as u64);
        let current_time = self.context.time.position_seconds();
        
        // Обновляем все серво
        for servo in self.servos.values_mut() {
            if let Some(signal) = servo.update(current_time) {
                // Отправляем изменение параметра в аудиограф
                let cmd = AutomationCommandEnum::SetParameter(signal);
                let _ = self.command_queue.send(cmd);
                self.stats.commands_processed += 1;
            }
        }
        
        // Обрабатываем входящие команды (из control-потока)
        while let Ok(cmd) = self.command_queue.try_recv() {
            match cmd {
                AutomationCommandEnum::SetParameter(signal) => {
                    // Передаем в аудиограф (будет реализовано позже)
                    self.stats.commands_processed += 1;
                }
                AutomationCommand::SetServo { id, enabled } => {
                    if let Some(servo) = self.servos.get_mut(&id) {
                        servo.set_enabled(enabled);
                    }
                }
                AutomationCommand::AddSensor { id, source, processor } => {
                    // Создаем сенсор по имени процессора
                    log::info!("Adding sensor: {}", id);
                }
                AutomationCommand::RemoveSensor { id } => {
                    self.sensors.remove(&id);
                    self.stats.sensors_active = self.sensors.len();
                }
                AutomationCommand::Shutdown => {
                    self.running = false;
                }
                _ => {}
            }
        }
    }
    
    /// Получить очередь команд (для audio-потока)
    pub fn command_queue(&self) -> CommandQueue {
        self.command_queue.clone()
    }
    
    /// Получить очередь телеметрии (для audio-потока)
    pub fn telemetry_queue(&self) -> TelemetryQueue {
        self.telemetry_queue.clone()
    }
    
    /// Получить статистику
    pub fn stats(&self) -> AutomationStats {
        self.stats
    }
}

impl Drop for AutomationManager {
    fn drop(&mut self) {
        self.stop();
    }
}

5. Обновим src/lib.rs:
rust

//! # Kama Automation - объединенная система автоматизации и управления
//!
//! Этот крейт предоставляет:
//!
//! ## Автоматизация (Automation)
//! - Генераторы управляющих сигналов (LFO, envelope, random walk)
//! - Серво для применения сигналов к параметрам
//! - Менеджер для синхронизации во времени
//!
//! ## Управление (Control)
//! - Маппинг событий контроллеров на параметры
//! - Двухпоточная архитектура с неблокирующими очередями
//! - Сенсоры для обратной связи от аудиографа
//!
//! ## Обратная связь (Feedback)
//! - Замкнутые системы управления
//! - Адаптивная автоматизация на основе состояния

#![warn(missing_docs)]

pub mod automaton;
pub mod servo;
pub mod sensor;
pub mod control;
pub mod core;
pub mod queues;
pub mod manager;

// Реэкспорты для удобства
pub use automaton::{
    Automaton,
    FunctionAutomaton,
    LfoAutomaton,
    LfoWaveform,
    StatefulFunctionAutomaton,
};
pub use servo::{
    Servo,
    ParameterMapping,
    AnyServo,
};
pub use sensor::{
    Sensor,
    SensorManager,
    SensorProcessor,
    IdentityProcessor,
    RmsProcessor,
    PeakProcessor,
};
pub use control::{
    ControlEngine,
    ControlEvent,
    EventPattern,
    Mapping,
    Target,
    Transform,
};
pub use core::{
    AutomationContext,
    ParameterTarget,
    SensorSource,
    SignalSource,
};
pub use manager::{
    AutomationManager,
    AutomationStats,
};
pub use queues::{
    AutomationCommand,
    TelemetryData,
    CommandQueue,
    TelemetryQueue,
};