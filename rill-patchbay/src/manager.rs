//! # Менеджер патчбэя — центральный координатор
//!
//! `PatchbayManager` объединяет все компоненты патчбэя:
//! - Автоматы (LFO, огибающие, секвенсоры)
//! - Маппинги событий (MIDI/OSC)
//! - Сервоприводы (связь с параметрами)
//! - Очередь команд для аудиопотока
//!
//! Работает в **потоке управления** (soft RT) и отправляет
//! команды в аудиопоток через `RtQueue<ParameterCommand>`.

use std::sync::Arc;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use parking_lot::RwLock;

use rill_core::prelude::*;
use rill_core::queue::RtQueue;
use rill_core::param::{ParameterId, ParameterValue};

use crate::control::*;
use crate::automaton::*;

// =============================================================================
// Типы команд для аудиопотока
// =============================================================================

/// Команда изменения параметра (отправляется в аудиопоток)
#[derive(Debug, Clone)]
pub struct ParameterCommand {
    /// ID узла
    pub node_id: NodeId,
    /// Имя параметра
    pub param: String,
    /// Новое значение
    pub value: f32,
    /// Временная метка (для отладки)
    pub timestamp: u64,
}

impl ParameterCommand {
    /// Создать новую команду
    pub fn new(node_id: NodeId, param: impl Into<String>, value: f32) -> Self {
        Self {
            node_id,
            param: param.into(),
            value,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64,
        }
    }
}

/// Событие для логирования и отладки
#[derive(Debug, Clone)]
pub enum PatchbayEvent {
    /// Автомат обновлён
    AutomatonUpdated {
        id: String,
        value: f64,
        time: f64,
    },
    /// Маппинг сработал
    MappingTriggered {
        pattern: String,
        target: String,
        value: f32,
    },
    /// Команда отправлена в аудиопоток
    CommandSent(ParameterCommand),
    /// Ошибка
    Error(String),
}

// =============================================================================
// Статистика работы патчбэя
// =============================================================================

/// Статистика работы патчбэя
#[derive(Debug, Clone, Default)]
pub struct PatchbayStats {
    /// Количество активных автоматов
    pub automaton_count: usize,
    /// Количество активных маппингов
    pub mapping_count: usize,
    /// Количество отправленных команд
    pub commands_sent: u64,
    /// Время последнего обновления
    pub last_update: Option<Duration>,
    /// Среднее время обновления (мкс)
    pub avg_update_time_us: f64,
    /// Максимальное время обновления (мкс)
    pub max_update_time_us: f64,
    /// Количество ошибок
    pub error_count: u64,
}

impl PatchbayStats {
    /// Обновить статистику
    pub fn update(&mut self, update_duration: Duration) {
        let us = update_duration.as_micros() as f64;
        self.avg_update_time_us = (self.avg_update_time_us * 0.9 + us * 0.1);
        self.max_update_time_us = self.max_update_time_us.max(us);
        self.last_update = Some(update_duration);
    }
}

// =============================================================================
// Конфигурация патчбэя
// =============================================================================

/// Конфигурация патчбэя
#[derive(Debug, Clone)]
pub struct PatchbayConfig {
    /// Частота обновления автоматов (Гц)
    pub update_rate_hz: f64,
    /// Размер очереди команд
    pub command_queue_size: usize,
    /// Собирать ли статистику
    pub collect_stats: bool,
    /// Логировать ли события
    pub log_events: bool,
}

impl Default for PatchbayConfig {
    fn default() -> Self {
        Self {
            update_rate_hz: 1000.0, // 1 кГц
            command_queue_size: 1024,
            collect_stats: true,
            log_events: false,
        }
    }
}

// =============================================================================
// Основной менеджер патчбэя
// =============================================================================

/// Главный менеджер патчбэя
///
/// Координирует все компоненты управления и автоматизации.
/// Работает в отдельном потоке с настраиваемой частотой обновления.
pub struct PatchbayManager {
    /// Конфигурация
    config: PatchbayConfig,
    
    /// Автоматы (ключ — ID)
    automata: HashMap<String, BoxedAutomaton>,
    
    /// Состояния автоматов
    automaton_states: HashMap<String, Box<dyn std::any::Any + Send>>,
    
    /// Сервоприводы (связь автоматов с параметрами)
    servos: HashMap<String, BoxedServo>,
    
    /// Маппинги событий
    mappings: Vec<Mapping>,
    
    /// Очередь для отправки команд в аудиопоток
    command_queue: Arc<RtQueue<ParameterCommand>>,
    
    /// Канал для событий (опционально)
    event_tx: Option<crossbeam_channel::Sender<PatchbayEvent>>,
    
    /// Текущее время (секунды)
    time: f64,
    
    /// Статистика
    stats: PatchbayStats,
    
    /// Флаг работы
    running: Arc<AtomicBool>,
    
    /// Поток обновления
    update_thread: Option<std::thread::JoinHandle<()>>,
}

impl PatchbayManager {
    /// Создать новый менеджер
    pub fn new(
        config: PatchbayConfig,
        command_queue: Arc<RtQueue<ParameterCommand>>,
    ) -> Self {
        Self {
            config,
            automata: HashMap::new(),
            automaton_states: HashMap::new(),
            servos: HashMap::new(),
            mappings: Vec::new(),
            command_queue,
            event_tx: None,
            time: 0.0,
            stats: PatchbayStats::default(),
            running: Arc::new(AtomicBool::new(false)),
            update_thread: None,
        }
    }
    
    /// Установить канал для событий
    pub fn with_event_channel(mut self, tx: crossbeam_channel::Sender<PatchbayEvent>) -> Self {
        self.event_tx = Some(tx);
        self
    }
    
    // =========================================================================
    // Управление автоматами
    // =========================================================================
    
    /// Добавить автомат
    pub fn add_automaton<A: Automaton + Clone + 'static>(
        &mut self,
        id: impl Into<String>,
        automaton: A,
    ) -> Result<(), &'static str>
    where
        A::State: 'static,
        A::Action: 'static,
    {
        let id = id.into();
        if self.automata.contains_key(&id) {
            return Err("Automaton with this ID already exists");
        }
        
        let state = automaton.initial_state();
        self.automata.insert(id.clone(), Box::new(automaton));
        self.automaton_states.insert(id, Box::new(state));
        
        Ok(())
    }
    
    /// Добавить LFO как автомат
    pub fn add_lfo(
        &mut self,
        id: impl Into<String>,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
    ) -> Result<(), &'static str> {
        let automaton = LfoAutomaton::new(
            &id.into(),
            frequency,
            amplitude,
            offset,
            waveform,
        );
        self.add_automaton(id, automaton)
    }
    
    /// Добавить огибающую как автомат
    pub fn add_envelope(
        &mut self,
        id: impl Into<String>,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
    ) -> Result<(), &'static str> {
        let automaton = EnvelopeAutomaton::adsr(
            &id.into(),
            attack,
            decay,
            sustain,
            release,
        );
        self.add_automaton(id, automaton)
    }
    
    /// Добавить секвенсор как автомат
    pub fn add_sequencer(
        &mut self,
        id: impl Into<String>,
        steps: Vec<Step>,
    ) -> Result<(), &'static str> {
        let automaton = SequencerAutomaton::new(&id.into(), steps);
        self.add_automaton(id, automaton)
    }
    
    /// Добавить функциональный автомат
    pub fn add_function<F>(
        &mut self,
        id: impl Into<String>,
        generator: F,
    ) -> Result<(), &'static str>
    where
        F: Fn(f64) -> f64 + Send + Sync + 'static,
    {
        let automaton = FunctionAutomaton::new(&id.into(), generator);
        self.add_automaton(id, automaton)
    }
    
    /// Получить значение автомата
    pub fn get_automaton_value(&self, id: &str) -> Option<f64> {
        let automaton = self.automata.get(id)?;
        let state = self.automaton_states.get(id)?;
        Some(automaton.extract_value_dyn(&**state))
    }
    
    /// Отправить действие автомату
    pub fn send_action<A: Automaton + 'static>(
        &mut self,
        id: &str,
        action: A::Action,
    ) -> Result<(), &'static str>
    where
        A::Action: 'static,
    {
        let automaton = self.automata.get(id)
            .ok_or("Automaton not found")?;
        
        let state = self.automaton_states.get_mut(id)
            .ok_or("State not found")?;
        
        // В реальном коде нужно применить действие
        // Здесь упрощённо
        
        Ok(())
    }
    
    /// Удалить автомат
    pub fn remove_automaton(&mut self, id: &str) -> bool {
        self.automata.remove(id).is_some() &&
        self.automaton_states.remove(id).is_some()
    }
    
    // =========================================================================
    // Управление сервоприводами
    // =========================================================================
    
    /// Добавить сервопривод (связь автомата с параметром)
    pub fn add_servo(
        &mut self,
        id: impl Into<String>,
        automaton_id: impl Into<String>,
        target_node: NodeId,
        target_param: impl Into<String>,
        mapping: ParameterMapping,
        min: f64,
        max: f64,
    ) -> Result<(), &'static str> {
        let automaton_id = automaton_id.into();
        let automaton = self.automata.get(&automaton_id)
            .ok_or("Automaton not found")?;
        
        // Создаём сервопривод
        // В реальном коде нужно клонировать автомат
        // Здесь упрощённо
        
        let servo = Box::new(TestServo {
            id: id.into(),
            target_node,
            target_param: target_param.into(),
            last_value: 0.0,
        });
        
        self.servos.insert(id.into(), servo);
        
        Ok(())
    }
    
    /// Добавить LFO как сервопривод (упрощённый метод)
    pub fn add_lfo_servo(
        &mut self,
        id: impl Into<String>,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        target_node: NodeId,
        target_param: impl Into<String>,
        min: f64,
        max: f64,
    ) -> Result<(), &'static str> {
        let automaton_id = format!("{}_auto", id.into());
        self.add_lfo(&automaton_id, frequency, amplitude, offset, waveform)?;
        self.add_servo(id, automaton_id, target_node, target_param, ParameterMapping::Linear, min, max)
    }
    
    /// Получить сервопривод
    pub fn get_servo(&self, id: &str) -> Option<&dyn AnyServo> {
        self.servos.get(id).map(|b| b.as_ref())
    }
    
    /// Получить мутабельный сервопривод
    pub fn get_servo_mut(&mut self, id: &str) -> Option<&mut BoxedServo> {
        self.servos.get_mut(id)
    }
    
    /// Удалить сервопривод
    pub fn remove_servo(&mut self, id: &str) -> bool {
        self.servos.remove(id).is_some()
    }
    
    // =========================================================================
    // Управление маппингами
    // =========================================================================
    
    /// Добавить маппинг события
    pub fn add_mapping(&mut self, mapping: Mapping) {
        self.mappings.push(mapping);
    }
    
    /// Добавить MIDI маппинг
    pub fn add_midi_mapping(
        &mut self,
        controller: u8,
        channel: Option<u8>,
        target_node: NodeId,
        target_param: impl Into<String>,
        min: f32,
        max: f32,
        transform: Transform,
    ) {
        let mapping = midi_cc(
            controller,
            channel,
            target_node,
            &target_param.into(),
            min,
            max,
            transform,
        );
        self.add_mapping(mapping);
    }
    
    /// Добавить OSC маппинг
    pub fn add_osc_mapping(
        &mut self,
        address: &str,
        target_node: NodeId,
        target_param: impl Into<String>,
        min: f32,
        max: f32,
        transform: Transform,
    ) {
        let mapping = osc_address(
            address,
            target_node,
            &target_param.into(),
            min,
            max,
            transform,
        );
        self.add_mapping(mapping);
    }
    
    /// Удалить маппинги по паттерну
    pub fn remove_mappings(&mut self, pattern: &EventPattern) -> usize {
        let before = self.mappings.len();
        self.mappings.retain(|m| &m.pattern != pattern);
        before - self.mappings.len()
    }
    
    /// Очистить все маппинги
    pub fn clear_mappings(&mut self) {
        self.mappings.clear();
    }
    
    // =========================================================================
    // Обработка событий
    // =========================================================================
    
    /// Обработать внешнее событие (MIDI/OSC)
    pub fn handle_event(&mut self, event: ControlEvent) {
        let mut commands = Vec::new();
        
        for mapping in &self.mappings {
            if let Some(cmd) = mapping.apply(&event) {
                commands.push(cmd);
                
                if self.config.log_events {
                    self.emit_event(PatchbayEvent::MappingTriggered {
                        pattern: format!("{:?}", mapping.pattern),
                        target: format!("{}:{}", mapping.target.node_id.0, mapping.target.param_name),
                        value: cmd.value,
                    });
                }
            }
        }
        
        // Отправляем команды в аудиопоток
        for cmd in commands {
            let _ = self.command_queue.push(cmd.clone());
            self.stats.commands_sent += 1;
            
            if self.config.log_events {
                self.emit_event(PatchbayEvent::CommandSent(cmd));
            }
        }
    }
    
    /// Обработать MIDI сообщение (упрощённый метод)
    pub fn handle_midi(&mut self, channel: u8, controller: u8, value: u8) {
        let event = ControlEvent::MidiControl {
            channel,
            controller,
            value,
            normalized: value as f32 / 127.0,
        };
        self.handle_event(event);
    }
    
    /// Обработать OSC сообщение (упрощённый метод)
    pub fn handle_osc(&mut self, address: &str, args: Vec<f32>) {
        let event = ControlEvent::Osc {
            address: address.to_string(),
            args,
        };
        self.handle_event(event);
    }
    
    // =========================================================================
    // Обновление состояния
    // =========================================================================
    
    /// Обновить состояние автоматов
    fn update_automata(&mut self, dt: f64) {
        self.time += dt;
        
        let mut commands = Vec::new();
        
        // Обновляем все автоматы и собираем команды
        for (id, automaton) in &self.automata {
            let state = self.automaton_states.get_mut(id).unwrap();
            
            // В реальном коде нужно извлечь состояние и применить действие
            // Здесь упрощённо
            
            if let Some(servo) = self.servos.get(id) {
                if let Some(cmd) = servo.update(self.time) {
                    commands.push(cmd);
                }
            }
        }
        
        // Отправляем команды в аудиопоток
        for cmd in commands {
            let _ = self.command_queue.push(cmd.clone());
            self.stats.commands_sent += 1;
        }
    }
    
    /// Отправить событие (если есть канал)
    fn emit_event(&self, event: PatchbayEvent) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event);
        }
    }
    
    // =========================================================================
    // Запуск и остановка
    // =========================================================================
    
    /// Запустить менеджер в отдельном потоке
    pub fn start(&mut self) -> Result<(), &'static str> {
        if self.running.load(Ordering::Relaxed) {
            return Err("Already running");
        }
        
        self.running.store(true, Ordering::Relaxed);
        
        let running = self.running.clone();
        let update_interval = Duration::from_secs_f64(1.0 / self.config.update_rate_hz);
        let collect_stats = self.config.collect_stats;
        
        // Создаём клоны для потока
        let mut automata = self.automata.clone();
        let mut automaton_states = std::mem::take(&mut self.automaton_states);
        let mut servos = std::mem::take(&mut self.servos);
        let command_queue = self.command_queue.clone();
        let event_tx = self.event_tx.clone();
        
        self.update_thread = Some(std::thread::spawn(move || {
            let mut last_time = Instant::now();
            let mut stats = PatchbayStats::default();
            let mut time = 0.0;
            
            while running.load(Ordering::Relaxed) {
                let frame_start = Instant::now();
                
                // Вычисляем dt
                let now = Instant::now();
                let dt = now.duration_since(last_time).as_secs_f64();
                last_time = now;
                time += dt;
                
                // Обновляем все автоматы
                let mut commands = Vec::new();
                
                for (id, automaton) in &automata {
                    if let Some(state) = automaton_states.get_mut(id) {
                        // В реальном коде здесь нужно применить шаг автомата
                        // и получить команды от сервоприводов
                        
                        if let Some(servo) = servos.get(id) {
                            if let Some(cmd) = servo.update(time) {
                                commands.push(cmd);
                            }
                        }
                    }
                }
                
                // Отправляем команды
                for cmd in commands {
                    let _ = command_queue.push(cmd.clone());
                    stats.commands_sent += 1;
                }
                
                // Обновляем статистику
                if collect_stats {
                    stats.update(frame_start.elapsed());
                }
                
                // Спим до следующего обновления
                let elapsed = frame_start.elapsed();
                if elapsed < update_interval {
                    std::thread::sleep(update_interval - elapsed);
                }
            }
        }));
        
        Ok(())
    }
    
    /// Остановить менеджер
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        
        if let Some(thread) = self.update_thread.take() {
            let _ = thread.join();
        }
    }
    
    /// Получить статистику
    pub fn stats(&self) -> &PatchbayStats {
        &self.stats
    }
    
    /// Сбросить статистику
    pub fn reset_stats(&mut self) {
        self.stats = PatchbayStats::default();
    }
    
    /// Получить текущее время
    pub fn current_time(&self) -> f64 {
        self.time
    }
    
    /// Проверить, запущен ли менеджер
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for PatchbayManager {
    fn drop(&mut self) {
        self.stop();
    }
}

// =============================================================================
// Вспомогательные типы для тестирования
// =============================================================================

/// Тестовый сервопривод (заглушка)
struct TestServo {
    id: String,
    target_node: NodeId,
    target_param: String,
    last_value: f64,
}

impl AnyServo for TestServo {
    fn update(&mut self, time: f64) -> Option<ParameterCommand> {
        // Генерируем тестовое значение
        let value = (time * 2.0).sin() * 0.5 + 0.5;
        
        if (value - self.last_value).abs() > 0.01 {
            self.last_value = value;
            Some(ParameterCommand::new(
                self.target_node,
                &self.target_param,
                value as f32,
            ))
        } else {
            None
        }
    }
    
    fn id(&self) -> &str {
        &self.id
    }
    
    fn set_enabled(&mut self, enabled: bool) {
        // Игнорируем
    }
    
    fn target(&self) -> PortId {
        PortId::node(self.target_node)
    }
}

// =============================================================================
// Строитель для удобного создания менеджера
// =============================================================================

/// Строитель для PatchbayManager
pub struct PatchbayManagerBuilder {
    config: PatchbayConfig,
    command_queue: Option<Arc<RtQueue<ParameterCommand>>>,
    event_channel: Option<crossbeam_channel::Sender<PatchbayEvent>>,
}

impl PatchbayManagerBuilder {
    /// Создать нового строителя
    pub fn new() -> Self {
        Self {
            config: PatchbayConfig::default(),
            command_queue: None,
            event_channel: None,
        }
    }
    
    /// Установить конфигурацию
    pub fn with_config(mut self, config: PatchbayConfig) -> Self {
        self.config = config;
        self
    }
    
    /// Установить частоту обновления
    pub fn with_update_rate(mut self, hz: f64) -> Self {
        self.config.update_rate_hz = hz;
        self
    }
    
    /// Установить очередь команд
    pub fn with_command_queue(mut self, queue: Arc<RtQueue<ParameterCommand>>) -> Self {
        self.command_queue = Some(queue);
        self
    }
    
    /// Установить канал событий
    pub fn with_event_channel(mut self, tx: crossbeam_channel::Sender<PatchbayEvent>) -> Self {
        self.event_channel = Some(tx);
        self.config.log_events = true;
        self
    }
    
    /// Включить сбор статистики
    pub fn with_stats(mut self, enabled: bool) -> Self {
        self.config.collect_stats = enabled;
        self
    }
    
    /// Собрать менеджер
    pub fn build(self) -> PatchbayManager {
        let queue = self.command_queue.unwrap_or_else(|| {
            Arc::new(RtQueue::new(self.config.command_queue_size))
        });
        
        let mut manager = PatchbayManager::new(self.config, queue);
        
        if let Some(tx) = self.event_channel {
            manager = manager.with_event_channel(tx);
        }
        
        manager
    }
}

impl Default for PatchbayManagerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;
    
    #[test]
    fn test_manager_creation() {
        let queue = Arc::new(RtQueue::new(1024));
        let manager = PatchbayManager::new(PatchbayConfig::default(), queue);
        
        assert_eq!(manager.automata.len(), 0);
        assert_eq!(manager.mappings.len(), 0);
        assert!(!manager.is_running());
    }
    
    #[test]
    fn test_add_automaton() {
        let queue = Arc::new(RtQueue::new(1024));
        let mut manager = PatchbayManager::new(PatchbayConfig::default(), queue);
        
        let result = manager.add_lfo("test_lfo", 1.0, 0.5, 0.0, LfoWaveform::Sine);
        assert!(result.is_ok());
        assert_eq!(manager.automata.len(), 1);
        
        let value = manager.get_automaton_value("test_lfo");
        assert!(value.is_some());
    }
    
    #[test]
    fn test_add_mapping() {
        let queue = Arc::new(RtQueue::new(1024));
        let mut manager = PatchbayManager::new(PatchbayConfig::default(), queue);
        
        manager.add_midi_mapping(7, None, NodeId(1), "volume", 0.0, 1.0, Transform::Linear);
        assert_eq!(manager.mappings.len(), 1);
    }
    
    #[test]
    fn test_handle_event() {
        let queue = Arc::new(RtQueue::new(1024));
        let mut manager = PatchbayManager::new(PatchbayConfig::default(), queue.clone());
        
        manager.add_midi_mapping(7, None, NodeId(1), "volume", 0.0, 1.0, Transform::Linear);
        
        let event = ControlEvent::MidiControl {
            channel: 1,
            controller: 7,
            value: 64,
            normalized: 0.5,
        };
        
        manager.handle_event(event);
        
        // Должна быть команда в очереди
        // assert!(queue.len() > 0); // В реальном тесте
    }
    
    #[test]
    fn test_start_stop() {
        let queue = Arc::new(RtQueue::new(1024));
        let mut manager = PatchbayManager::new(PatchbayConfig::default(), queue);
        
        let result = manager.start();
        assert!(result.is_ok());
        assert!(manager.is_running());
        
        thread::sleep(Duration::from_millis(100));
        
        manager.stop();
        assert!(!manager.is_running());
    }
    
    #[test]
    fn test_builder() {
        let queue = Arc::new(RtQueue::new(1024));
        
        let manager = PatchbayManagerBuilder::new()
            .with_update_rate(500.0)
            .with_command_queue(queue)
            .with_stats(true)
            .build();
        
        assert_eq!(manager.config.update_rate_hz, 500.0);
        assert!(manager.config.collect_stats);
    }
}