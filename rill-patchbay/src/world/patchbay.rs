//! Patchbay — мир, где живут автоматы

use crate::control::Automaton;
use crate::core::{WorldSignal, WorldTime};
use crate::sensor::Sensor;
use crate::servo::{SafeServo, WildServo};
use rill_core::queues::{Command, MicroControlObserver, TelemetryQueue};
use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Мир автоматов
pub struct Patchbay {
    /// Имя мира
    name: String,
    
    /// Автоматы (разум)
    automata: HashMap<String, Box<dyn Automaton>>,
    
    /// Сенсоры (чувства)
    sensors: HashMap<String, Box<dyn Sensor>>,
    
    /// Безопасные серво (через очереди)
    safe_servos: HashMap<String, Box<dyn crate::servo::SafeServoHandle>>,
    
    /// "Дикие" серво (микро-контроль)
    wild_servos: HashMap<String, Box<dyn crate::servo::WildServoHandle>>,
    
    /// Наблюдатель
    observer: MicroControlObserver,
    
    /// Очередь телеметрии (откуда получаем данные о звуке)
    telemetry_rx: crossbeam_channel::Receiver<rill_core::queues::Telemetry>,
    
    /// Очередь команд (куда отправляем команды в граф)
    cmd_tx: crossbeam_channel::Sender<Command>,
    
    /// Поток выполнения
    thread: Option<thread::JoinHandle<()>>,
    
    /// Флаг работы
    running: bool,
}

impl Patchbay {
    pub fn new(
        name: impl Into<String>,
        telemetry_rx: crossbeam_channel::Receiver<rill_core::queues::Telemetry>,
        cmd_tx: crossbeam_channel::Sender<Command>,
        observer: MicroControlObserver,
    ) -> Self {
        Self {
            name: name.into(),
            automata: HashMap::new(),
            sensors: HashMap::new(),
            safe_servos: HashMap::new(),
            wild_servos: HashMap::new(),
            observer,
            telemetry_rx,
            cmd_tx,
            thread: None,
            running: false,
        }
    }
    
    /// Добавить автомат
    pub fn add_automaton(&mut self, automaton: Box<dyn Automaton>) {
        let name = automaton.name().to_string();
        self.automata.insert(name, automaton);
    }
    
    /// Добавить сенсор
    pub fn add_sensor(&mut self, sensor: Box<dyn Sensor>) {
        let name = sensor.name().to_string();
        self.sensors.insert(name, sensor);
    }
    
    /// Добавить безопасное серво
    pub fn add_safe_servo<A: Automaton + 'static>(
        &mut self,
        servo: SafeServo<A>,
    ) {
        struct Wrapper<A: Automaton> {
            servo: SafeServo<A>,
        }
        
        impl<A: Automaton> crate::servo::SafeServoHandle for Wrapper<A> {
            fn update(&mut self, time: WorldTime) {
                self.servo.update(time);
            }
            
            fn name(&self) -> &str {
                &self.servo.name
            }
        }
        
        let name = servo.name().to_string();
        self.safe_servos.insert(name, Box::new(Wrapper { servo }));
    }
    
    /// Добавить "дикое" серво
    pub fn add_wild_servo<A: Automaton + 'static>(
        &mut self,
        servo: WildServo<A>,
    ) {
        struct Wrapper<A: Automaton> {
            servo: WildServo<A>,
        }
        
        impl<A: Automaton> crate::servo::WildServoHandle for Wrapper<A> {
            fn update(&mut self, time: WorldTime, graph: &mut rill_graph::AudioGraph) {
                self.servo.update(time, graph);
            }
            
            fn name(&self) -> &str {
                &self.servo.name
            }
        }
        
        let name = servo.name().to_string();
        self.wild_servos.insert(name, Box::new(Wrapper { servo }));
    }
    
    /// Запустить мир
    pub fn awaken(&mut self) {
        if self.running {
            return;
        }
        
        self.running = true;
        
        let mut automata = std::mem::take(&mut self.automata);
        let mut sensors = std::mem::take(&mut self.sensors);
        let mut safe_servos = std::mem::take(&mut self.safe_servos);
        let telemetry_rx = self.telemetry_rx.clone();
        
        let handle = thread::spawn(move || {
            let tick_interval = Duration::from_micros(1000); // 1kHz
            let mut last_tick = std::time::Instant::now();
            let mut time = WorldTime::new(44100.0); // 44.1kHz тиков
            
            while let Ok(()) = Self::tick(
                &mut automata,
                &mut sensors,
                &mut safe_servos,
                &telemetry_rx,
                &mut time,
            ) {
                let now = std::time::Instant::now();
                let elapsed = now.duration_since(last_tick);
                if elapsed < tick_interval {
                    thread::sleep(tick_interval - elapsed);
                }
                last_tick = now;
            }
        });
        
        self.thread = Some(handle);
    }
    
    /// Один тик жизни мира
    fn tick(
        automata: &mut HashMap<String, Box<dyn Automaton>>,
        sensors: &mut HashMap<String, Box<dyn Sensor>>,
        safe_servos: &mut HashMap<String, Box<dyn crate::servo::SafeServoHandle>>,
        telemetry_rx: &crossbeam_channel::Receiver<rill_core::queues::Telemetry>,
        time: &mut WorldTime,
    ) -> Result<(), ()> {
        time.tick();
        
        // Собираем сигналы от сенсоров
        let mut signals = Vec::new();
        
        // Обрабатываем телеметрию из звукового мира
        while let Ok(telemetry) = telemetry_rx.try_recv() {
            for sensor in sensors.values_mut() {
                if let Some(signal) = sensor.process_telemetry(&telemetry) {
                    signals.push(signal);
                }
            }
        }
        
        // Автоматы обрабатывают сигналы
        let mut new_signals = Vec::new();
        for automaton in automata.values_mut() {
            let output = automaton.process(*time, &signals);
            new_signals.extend(output);
        }
        signals.extend(new_signals);
        
        // Безопасные серво отправляют команды
        for servo in safe_servos.values_mut() {
            servo.update(*time);
        }
        
        Ok(())
    }
    
    /// Остановить мир
    pub fn rest(&mut self) {
        self.running = false;
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}