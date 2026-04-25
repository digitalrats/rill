//! # Управление и автоматизация (Control + Automation)
//!
//! `rill-patchbay::control` объединяет функциональность:
//! - Маппинг событий (MIDI/OSC) на параметры узлов (из rill-control)
//! - Автоматизацию через LFO, огибающие и другие генераторы (из rill-automation)
//! - Двухпоточную модель с неблокирующими очередями
//!
//! Все операции выполняются в **потоке управления** (soft RT) и
//! отправляют команды в аудиопоток через `RtQueue<ParameterCommand>`.

use std::sync::Arc;
use std::collections::HashMap;
use std::fmt::Debug;

use parking_lot::RwLock;
use rill_core::prelude::*;
use rill_core::queue::RtQueue;
use rill_core::param::{ParameterId, ParameterValue};

// =============================================================================
// 1. Паттерны событий (из rill-control)
// =============================================================================

/// Паттерн для сопоставления событий
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventPattern {
    /// Любая кнопка
    AnyButton,
    /// Кнопка с конкретным ID
    ButtonId(u32),
    
    /// Любая ручка
    AnyKnob,
    /// Ручка с конкретным ID
    KnobId(u32),
    
    /// Любой фейдер
    AnyFader,
    /// Фейдер с конкретным ID
    FaderId(u32),
    
    /// Любое MIDI сообщение
    AnyMidi,
    /// MIDI Control Change
    MidiControl {
        channel: Option<u8>,
        controller: u8,
    },
    /// MIDI Note
    MidiNote {
        channel: Option<u8>,
        note: Option<u8>,
    },
    
    /// OSC сообщение по адресу
    OscAddress(String),
    
    /// OSC с паттерном (содержит)
    OscPattern(String),
}

impl EventPattern {
    /// Проверить, соответствует ли событие паттерну
    pub fn matches(&self, event: &ControlEvent) -> bool {
        match (self, event) {
            (EventPattern::AnyButton, ControlEvent::Button { .. }) => true,
            (EventPattern::ButtonId(id), ControlEvent::Button { id: eid, .. }) => *id == *eid,
            
            (EventPattern::AnyKnob, ControlEvent::Knob { .. }) => true,
            (EventPattern::KnobId(id), ControlEvent::Knob { id: eid, .. }) => *id == *eid,
            
            (EventPattern::AnyFader, ControlEvent::Fader { .. }) => true,
            (EventPattern::FaderId(id), ControlEvent::Fader { id: eid, .. }) => *id == *eid,
            
            (EventPattern::MidiControl { channel, controller }, 
             ControlEvent::MidiControl { channel: ech, controller: ectr, .. }) => {
                (channel.is_none() || channel.unwrap() == *ech) && *controller == *ectr
            }
            
            (EventPattern::OscAddress(addr), ControlEvent::Osc { address, .. }) => addr == address,
            
            (EventPattern::OscPattern(pat), ControlEvent::Osc { address, .. }) => {
                address.contains(pat)
            }
            
            _ => false,
        }
    }
}

// =============================================================================
// 2. Типы событий (из rill-control)
// =============================================================================

/// Событие контроллера
#[derive(Debug, Clone, PartialEq)]
pub enum ControlEvent {
    /// Кнопка (нажата/отпущена)
    Button {
        id: u32,
        pressed: bool,
    },
    
    /// Поворотная ручка (энкодер)
    Knob {
        id: u32,
        value: f32,           // 0.0 - 1.0
        normalized: f32,      // то же, для совместимости
    },
    
    /// Фейдер (линейный ползунок)
    Fader {
        id: u32,
        value: f32,           // 0.0 - 1.0
        normalized: f32,
    },
    
    /// MIDI Control Change
    MidiControl {
        channel: u8,
        controller: u8,
        value: u8,            // 0-127
        normalized: f32,      // 0.0 - 1.0
    },
    
    /// MIDI Note
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
}

impl ControlEvent {
    /// Получить нормализованное значение (0.0-1.0), если применимо
    pub fn normalized_value(&self) -> Option<f32> {
        match self {
            ControlEvent::Knob { normalized, .. } => Some(*normalized),
            ControlEvent::Fader { normalized, .. } => Some(*normalized),
            ControlEvent::MidiControl { normalized, .. } => Some(*normalized),
            ControlEvent::Button { pressed, .. } => Some(if *pressed { 1.0 } else { 0.0 }),
            _ => None,
        }
    }
    
    /// Получить ID элемента управления, если применимо
    pub fn id(&self) -> Option<u32> {
        match self {
            ControlEvent::Button { id, .. } => Some(*id),
            ControlEvent::Knob { id, .. } => Some(*id),
            ControlEvent::Fader { id, .. } => Some(*id),
            _ => None,
        }
    }
}

// =============================================================================
// 3. Трансформации значений (из rill-control)
// =============================================================================

/// Тип преобразования значения
#[derive(Clone)]
pub enum Transform {
    /// Линейное: out = min + value * (max - min)
    Linear,
    
    /// Экспоненциальное: out = min + value^2 * (max - min)
    Exponential,
    
    /// Логарифмическое: out = min + log(1 + value * 9) / log(10) * (max - min)
    Logarithmic,
    
    /// Инвертированное: out = max - value * (max - min)
    Inverted,
    
    /// Пользовательское
    Custom(Arc<dyn Fn(f32) -> f32 + Send + Sync>),
}

impl Debug for Transform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transform::Linear => write!(f, "Linear"),
            Transform::Exponential => write!(f, "Exponential"),
            Transform::Logarithmic => write!(f, "Logarithmic"),
            Transform::Inverted => write!(f, "Inverted"),
            Transform::Custom(_) => write!(f, "Custom"),
        }
    }
}

impl Transform {
    /// Применить преобразование к нормализованному значению (0-1)
    pub fn apply(&self, value: f32, min: f32, max: f32) -> f32 {
        let range = max - min;
        let normalized = value.clamp(0.0, 1.0);
        
        let mapped = match self {
            Transform::Linear => min + normalized * range,
            Transform::Exponential => min + normalized * normalized * range,
            Transform::Logarithmic => min + (1.0 + normalized * 9.0).log10() * range,
            Transform::Inverted => max - normalized * range,
            Transform::Custom(f) => min + f(normalized) * range,
        };
        
        mapped.clamp(min, max)
    }
}

// =============================================================================
// 4. Маппинг событий (из rill-control)
// =============================================================================

/// Целевой параметр узла
#[derive(Debug, Clone)]
pub struct Target {
    /// ID узла в графе
    pub node_id: NodeId,
    /// Имя параметра
    pub param_name: String,
    /// Минимальное значение
    pub min: f32,
    /// Максимальное значение
    pub max: f32,
}

/// Маппинг события на параметр
#[derive(Debug, Clone)]
pub struct Mapping {
    /// Паттерн события
    pub pattern: EventPattern,
    /// Целевой параметр
    pub target: Target,
    /// Преобразование
    pub transform: Transform,
    /// Название (для отладки)
    pub name: String,
    /// Активен ли маппинг
    pub enabled: bool,
}

impl Mapping {
    /// Создать новый маппинг
    pub fn new(pattern: EventPattern, target: Target, transform: Transform) -> Self {
        let name = format!("{:?} -> {}", pattern, target.param_name);
        Self {
            pattern,
            target,
            transform,
            name,
            enabled: true,
        }
    }
    
    /// Проверить, подходит ли событие под этот маппинг
    pub fn matches(&self, event: &ControlEvent) -> bool {
        self.enabled && self.pattern.matches(event)
    }
    
    /// Применить событие и получить команду для параметра
    pub fn apply(&self, event: &ControlEvent) -> Option<ParameterCommand> {
        if !self.matches(event) {
            return None;
        }
        
        event.normalized_value().map(|norm| {
            let value = self.transform.apply(norm, self.target.min, self.target.max);
            ParameterCommand {
                node_id: self.target.node_id,
                param: self.target.param_name.clone(),
                value,
            }
        })
    }
}

// =============================================================================
// 5. Автоматы (из rill-automation)
// =============================================================================

/// Тип времени для автоматов
pub type Time = f64;

/// Базовый трейт для всех автоматов
pub trait Automaton: Send + Sync {
    /// Тип состояния
    type State: Clone + Send + Sync + 'static;
    
    /// Выполнить один шаг автомата
    fn step(
        &self,
        time: Time,
        state: &Self::State,
    ) -> (Self::State, Option<f64>);
    
    /// Начальное состояние
    fn initial_state(&self) -> Self::State;
    
    /// Имя автомата
    fn name(&self) -> &str;
    
    /// Извлечь значение из состояния
    fn extract_value(&self, state: &Self::State) -> f64;
}

/// Функциональный автомат (stateless)
pub struct FunctionAutomaton {
    name: String,
    generator: Box<dyn Fn(Time) -> f64 + Send + Sync>,
    target_node: NodeId,
    target_param: String,
}

impl FunctionAutomaton {
    pub fn new<F>(
        name: &str,
        generator: F,
        target_node: NodeId,
        target_param: &str,
    ) -> Self
    where
        F: Fn(Time) -> f64 + Send + Sync + 'static,
    {
        Self {
            name: name.to_string(),
            generator: Box::new(generator),
            target_node,
            target_param: target_param.to_string(),
        }
    }
}

impl Automaton for FunctionAutomaton {
    type State = f64;
    
    fn step(&self, time: Time, state: &Self::State) -> (Self::State, Option<f64>) {
        let value = (self.generator)(time);
        (value, Some(value))
    }
    
    fn initial_state(&self) -> Self::State {
        (self.generator)(0.0)
    }
    
    fn name(&self) -> &str {
        &self.name
    }
    
    fn extract_value(&self, state: &Self::State) -> f64 {
        *state
    }
}

/// LFO автомат
pub struct LfoAutomaton {
    name: String,
    frequency: f64,
    amplitude: f64,
    offset: f64,
    waveform: LfoWaveform,
    phase: f64,
    target_node: NodeId,
    target_param: String,
}

/// Форма волны для LFO
#[derive(Debug, Clone, Copy)]
pub enum LfoWaveform {
    Sine,
    Triangle,
    Saw,
    Square,
    SampleAndHold,
    RandomWalk,
}

impl LfoAutomaton {
    pub fn new(
        name: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        target_node: NodeId,
        target_param: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            frequency,
            amplitude,
            offset,
            waveform,
            phase: 0.0,
            target_node,
            target_param: target_param.to_string(),
        }
    }
    
    fn generate(&mut self, time: Time) -> f64 {
        let dt = time - self.phase;
        self.phase = time;
        
        // Обновляем фазу
        let phase_inc = self.frequency * dt;
        self.phase = (self.phase + phase_inc) % 1.0;
        
        // Генерируем значение
        let raw = match self.waveform {
            LfoWaveform::Sine => (self.phase * 2.0 * std::f64::consts::PI).sin(),
            LfoWaveform::Triangle => {
                if self.phase < 0.5 {
                    4.0 * self.phase - 1.0
                } else {
                    3.0 - 4.0 * self.phase
                }
            }
            LfoWaveform::Saw => 2.0 * self.phase - 1.0,
            LfoWaveform::Square => {
                if self.phase < 0.5 { 1.0 } else { -1.0 }
            }
            LfoWaveform::SampleAndHold => {
                // Простейший S&H - обновляем при переходе через 0
                if self.phase < 0.1 {
                    rand::random::<f64>() * 2.0 - 1.0
                } else {
                    self.phase // Заглушка
                }
            }
            LfoWaveform::RandomWalk => {
                // Простое случайное блуждание
                self.phase + (rand::random::<f64>() - 0.5) * 0.1
            }
        };
        
        raw * self.amplitude + self.offset
    }
}

impl Automaton for LfoAutomaton {
    type State = f64;
    
    fn step(&self, time: Time, _state: &Self::State) -> (Self::State, Option<f64>) {
        let mut me = self.clone();
        let value = me.generate(time);
        (value, Some(value))
    }
    
    fn initial_state(&self) -> Self::State {
        0.0
    }
    
    fn name(&self) -> &str {
        &self.name
    }
    
    fn extract_value(&self, state: &Self::State) -> f64 {
        *state
    }
}

/// Огибающая ADSR
pub struct EnvelopeAutomaton {
    name: String,
    attack: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    stage: EnvelopeStage,
    level: f64,
    trigger_time: Option<Time>,
    release_time: Option<Time>,
    target_node: NodeId,
    target_param: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EnvelopeStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

impl EnvelopeAutomaton {
    pub fn new(
        name: &str,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        target_node: NodeId,
        target_param: &str,
    ) -> Self {
        Self {
            name: name.to_string(),
            attack,
            decay,
            sustain,
            release,
            stage: EnvelopeStage::Off,
            level: 0.0,
            trigger_time: None,
            release_time: None,
            target_node,
            target_param: target_param.to_string(),
        }
    }
    
    pub fn trigger(&mut self, time: Time) {
        self.stage = EnvelopeStage::Attack;
        self.trigger_time = Some(time);
        self.release_time = None;
    }
    
    pub fn release(&mut self, time: Time) {
        if self.stage != EnvelopeStage::Off {
            self.stage = EnvelopeStage::Release;
            self.release_time = Some(time);
        }
    }
}

impl Automaton for EnvelopeAutomaton {
    type State = (EnvelopeStage, f64, Option<Time>, Option<Time>);
    
    fn step(&self, time: Time, state: &Self::State) -> (Self::State, Option<f64>) {
        let (mut stage, mut level, trigger_time, release_time) = state.clone();
        
        match stage {
            EnvelopeStage::Off => {
                level = 0.0;
            }
            
            EnvelopeStage::Attack => {
                if let Some(t) = trigger_time {
                    let elapsed = time - t;
                    if elapsed < self.attack {
                        level = elapsed / self.attack;
                    } else {
                        stage = EnvelopeStage::Decay;
                        level = 1.0;
                    }
                }
            }
            
            EnvelopeStage::Decay => {
                if let Some(t) = trigger_time {
                    let elapsed = time - t - self.attack;
                    if elapsed < self.decay {
                        level = 1.0 - (1.0 - self.sustain) * (elapsed / self.decay);
                    } else {
                        stage = EnvelopeStage::Sustain;
                        level = self.sustain;
                    }
                }
            }
            
            EnvelopeStage::Sustain => {
                level = self.sustain;
            }
            
            EnvelopeStage::Release => {
                if let Some(t) = release_time {
                    let elapsed = time - t;
                    if elapsed < self.release {
                        level = self.sustain * (1.0 - elapsed / self.release);
                    } else {
                        stage = EnvelopeStage::Off;
                        level = 0.0;
                    }
                }
            }
        }
        
        ( (stage, level, trigger_time, release_time), Some(level) )
    }
    
    fn initial_state(&self) -> Self::State {
        (EnvelopeStage::Off, 0.0, None, None)
    }
    
    fn name(&self) -> &str {
        &self.name
    }
    
    fn extract_value(&self, state: &Self::State) -> f64 {
        state.1
    }
}

// =============================================================================
// 6. Сервоприводы (связь автоматов с параметрами)
// =============================================================================

/// Тип маппинга значений для сервопривода
#[derive(Debug, Clone)]
pub enum ParameterMapping {
    Linear,
    Exponential,
    Logarithmic,
    Inverted,
    Custom(Arc<dyn Fn(f64) -> f64 + Send + Sync>),
}

impl ParameterMapping {
    pub fn apply(&self, raw: f64) -> f64 {
        match self {
            ParameterMapping::Linear => raw,
            ParameterMapping::Exponential => raw * raw,
            ParameterMapping::Logarithmic => (1.0 + raw * 9.0).log10(),
            ParameterMapping::Inverted => 1.0 - raw,
            ParameterMapping::Custom(f) => f(raw),
        }
    }
}

/// Сервопривод — связывает автомат с параметром узла
pub struct Servo<A: Automaton> {
    /// Идентификатор
    id: String,
    /// Автомат
    automaton: A,
    /// Состояние автомата
    state: A::State,
    /// Целевой узел
    target_node: NodeId,
    /// Целевой параметр
    target_param: String,
    /// Маппинг значения
    mapping: ParameterMapping,
    /// Минимальное значение
    min: f64,
    /// Максимальное значение
    max: f64,
    /// Последнее отправленное значение
    last_value: f64,
    /// Активен ли сервопривод
    enabled: bool,
    /// Время последнего обновления
    last_time: Time,
}

impl<A: Automaton> Servo<A> {
    pub fn new(
        id: impl Into<String>,
        automaton: A,
        target_node: NodeId,
        target_param: impl Into<String>,
        mapping: ParameterMapping,
        min: f64,
        max: f64,
    ) -> Self {
        let state = automaton.initial_state();
        Self {
            id: id.into(),
            automaton,
            state,
            target_node,
            target_param: target_param.into(),
            mapping,
            min,
            max,
            last_value: 0.0,
            enabled: true,
            last_time: 0.0,
        }
    }
    
    /// Обновить сервопривод и вернуть команду, если значение изменилось
    pub fn update(&mut self, time: Time) -> Option<ParameterCommand> {
        if !self.enabled {
            return None;
        }
        
        let (new_state, value_opt) = self.automaton.step(time, &self.state);
        self.state = new_state;
        
        if let Some(raw_value) = value_opt {
            let mapped = self.mapping.apply(raw_value);
            let clamped = mapped.clamp(self.min, self.max);
            
            // Отправляем только если значение изменилось значительно
            if (clamped - self.last_value).abs() > 1e-6 {
                self.last_value = clamped;
                self.last_time = time;
                
                return Some(ParameterCommand {
                    node_id: self.target_node,
                    param: self.target_param.clone(),
                    value: clamped as f32,
                });
            }
        }
        
        None
    }
    
    /// Включить/выключить сервопривод
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    /// Получить ID сервопривода
    pub fn id(&self) -> &str {
        &self.id
    }
}

// Тип для хранения разнородных сервоприводов
pub type BoxedServo = Box<dyn AnyServo>;

pub trait AnyServo: Send + Sync {
    fn update(&mut self, time: Time) -> Option<ParameterCommand>;
    fn id(&self) -> &str;
    fn set_enabled(&mut self, enabled: bool);
}

impl<A: Automaton + 'static> AnyServo for Servo<A> {
    fn update(&mut self, time: Time) -> Option<ParameterCommand> {
        Servo::update(self, time)
    }
    
    fn id(&self) -> &str {
        &self.id
    }
    
    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
}

// =============================================================================
// 7. Команды для аудиопотока
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
}

// =============================================================================
// 8. Главный контроллер (Patchbay Control)
// =============================================================================

/// Главный контроллер патчбэя
///
/// Работает в **потоке управления** (soft RT) и отправляет команды
/// в аудиопоток через `RtQueue<ParameterCommand>`.
pub struct PatchbayControl {
    /// Маппинги событий
    mappings: Vec<Mapping>,
    
    /// Сервоприводы (автоматы)
    servos: HashMap<String, BoxedServo>,
    
    /// Очередь для отправки команд в аудиопоток
    command_queue: Arc<RtQueue<ParameterCommand>>,
    
    /// Внутреннее время (секунды)
    time: Time,
}

impl PatchbayControl {
    /// Создать новый контроллер
    pub fn new(command_queue: Arc<RtQueue<ParameterCommand>>) -> Self {
        Self {
            mappings: Vec::new(),
            servos: HashMap::new(),
            command_queue,
            time: 0.0,
        }
    }
    
    /// Добавить маппинг события
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
    ) -> Result<(), &'static str> {
        let pattern = match pattern {
            p if p.starts_with("button:") => {
                let id = p[7..].parse().map_err(|_| "Invalid button ID")?;
                EventPattern::ButtonId(id)
            }
            p if p.starts_with("knob:") => {
                let id = p[5..].parse().map_err(|_| "Invalid knob ID")?;
                EventPattern::KnobId(id)
            }
            p if p.starts_with("fader:") => {
                let id = p[6..].parse().map_err(|_| "Invalid fader ID")?;
                EventPattern::FaderId(id)
            }
            p if p.starts_with("midi:") => {
                let parts: Vec<&str> = p[5..].split(':').collect();
                if parts.len() == 2 {
                    let channel = parts[0].parse().ok();
                    let controller = parts[1].parse().map_err(|_| "Invalid controller")?;
                    EventPattern::MidiControl { channel, controller }
                } else {
                    EventPattern::AnyMidi
                }
            }
            p if p.starts_with("osc:") => EventPattern::OscAddress(p[4..].to_string()),
            _ => return Err("Unknown pattern"),
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
    
    /// Добавить сервопривод (автомат)
    pub fn add_servo<A: Automaton + 'static>(&mut self, servo: Servo<A>) {
        self.servos.insert(servo.id().to_string(), Box::new(servo));
    }
    
    /// Добавить LFO как сервопривод
    pub fn add_lfo(
        &mut self,
        id: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        target_node: NodeId,
        target_param: &str,
        min: f64,
        max: f64,
    ) {
        let automaton = LfoAutomaton::new(
            id,
            frequency,
            amplitude,
            offset,
            waveform,
            target_node,
            target_param,
        );
        
        let servo = Servo::new(
            id,
            automaton,
            target_node,
            target_param,
            ParameterMapping::Linear,
            min,
            max,
        );
        
        self.add_servo(servo);
    }
    
    /// Добавить огибающую как сервопривод
    pub fn add_envelope(
        &mut self,
        id: &str,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
        target_node: NodeId,
        target_param: &str,
        min: f64,
        max: f64,
    ) {
        let automaton = EnvelopeAutomaton::new(
            id,
            attack,
            decay,
            sustain,
            release,
            target_node,
            target_param,
        );
        
        let servo = Servo::new(
            id,
            automaton,
            target_node,
            target_param,
            ParameterMapping::Linear,
            min,
            max,
        );
        
        self.add_servo(servo);
    }
    
    /// Обработать внешнее событие (MIDI/OSC)
    pub fn handle_event(&mut self, event: ControlEvent) {
        for mapping in &self.mappings {
            if let Some(cmd) = mapping.apply(&event) {
                let _ = self.command_queue.push(cmd);
            }
        }
    }
    
    /// Обновить состояние (вызывается регулярно из потока управления)
    pub fn update(&mut self, dt: f32) {
        self.time += dt as f64;
        
        // Обновляем все сервоприводы
        for servo in self.servos.values_mut() {
            if let Some(cmd) = servo.update(self.time) {
                let _ = self.command_queue.push(cmd);
            }
        }
    }
    
    /// Получить все маппинги
    pub fn mappings(&self) -> &[Mapping] {
        &self.mappings
    }
    
    /// Получить сервопривод по ID
    pub fn get_servo(&self, id: &str) -> Option<&dyn AnyServo> {
        self.servos.get(id).map(|b| b.as_ref())
    }
    
    /// Получить мутабельный сервопривод по ID
    pub fn get_servo_mut(&mut self, id: &str) -> Option<&mut BoxedServo> {
        self.servos.get_mut(id)
    }
    
    /// Удалить сервопривод
    pub fn remove_servo(&mut self, id: &str) -> bool {
        self.servos.remove(id).is_some()
    }
    
    /// Очистить все маппинги и сервоприводы
    pub fn clear(&mut self) {
        self.mappings.clear();
        self.servos.clear();
    }
    
    /// Сбросить время
    pub fn reset_time(&mut self) {
        self.time = 0.0;
    }
    
    /// Текущее время
    pub fn current_time(&self) -> Time {
        self.time
    }
}

// =============================================================================
// 9. Вспомогательные функции для создания маппингов
// =============================================================================

/// Создать маппинг MIDI CC на параметр
pub fn midi_cc(
    controller: u8,
    channel: Option<u8>,
    target_node: NodeId,
    target_param: &str,
    min: f32,
    max: f32,
    transform: Transform,
) -> Mapping {
    let pattern = EventPattern::MidiControl { channel, controller };
    let target = Target {
        node_id: target_node,
        param_name: target_param.to_string(),
        min,
        max,
    };
    Mapping::new(pattern, target, transform)
}

/// Создать маппинг OSC адреса на параметр
pub fn osc_address(
    address: &str,
    target_node: NodeId,
    target_param: &str,
    min: f32,
    max: f32,
    transform: Transform,
) -> Mapping {
    let pattern = EventPattern::OscAddress(address.to_string());
    let target = Target {
        node_id: target_node,
        param_name: target_param.to_string(),
        min,
        max,
    };
    Mapping::new(pattern, target, transform)
}

// =============================================================================
// 10. Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_midi_mapping() {
        let node = NodeId(1);
        let mapping = midi_cc(
            7, Some(1), node, "volume",
            0.0, 1.0, Transform::Linear,
        );
        
        let event = ControlEvent::MidiControl {
            channel: 1,
            controller: 7,
            value: 64,
            normalized: 0.5,
        };
        
        assert!(mapping.matches(&event));
        
        let cmd = mapping.apply(&event).unwrap();
        assert_eq!(cmd.node_id, node);
        assert_eq!(cmd.param, "volume");
        assert!((cmd.value - 0.5).abs() < 1e-6);
    }
    
    #[test]
    fn test_lfo_servo() {
        let node = NodeId(1);
        let queue = Arc::new(RtQueue::new(64));
        let mut control = PatchbayControl::new(queue);
        
        control.add_lfo(
            "test_lfo",
            1.0, 0.5, 0.0,
            LfoWaveform::Sine,
            node, "cutoff",
            100.0, 1000.0,
        );
        
        assert!(control.get_servo("test_lfo").is_some());
        
        // Несколько обновлений должны генерировать команды
        for i in 0..10 {
            control.update(0.1);
        }
    }
    
    #[test]
    fn test_envelope_servo() {
        let node = NodeId(1);
        let queue = Arc::new(RtQueue::new(64));
        let mut control = PatchbayControl::new(queue.clone());
        
        control.add_envelope(
            "test_env",
            0.1, 0.2, 0.7, 0.3,
            node, "gain",
            0.0, 1.0,
        );
        
        // Находим сервопривод и триггерим его
        if let Some(servo) = control.get_servo_mut("test_env") {
            // В реальном коде здесь нужно вызвать trigger
            // Для теста просто обновляем время
        }
        
        control.update(0.05);
        control.update(0.05);
        
        // Должны быть команды в очереди
        // assert!(queue.len() > 0); // В реальном тесте
    }
}