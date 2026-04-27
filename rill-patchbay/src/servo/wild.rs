//! "Дикое" серво — нарушает законы природы (микро-контроль)

use crate::control::Automaton;
use crate::core::{ParameterTarget, SignalOrigin, WorldTime};
use crate::servo::Servo;
use rill_core::queues::MicroControlObserver;
use rill_core::traits::{AudioNode, ParamValue, PortId};
use rill_graph::AudioGraph;
use std::sync::Arc;

/// "Дикое" серво — работает напрямую с графом, нарушая законы природы
///
/// Используется только для микро-контроля, когда каждая наносекунда на счету.
/// За его действиями следит наблюдатель.
pub struct WildServo<A: Automaton> {
    /// Имя серво
    name: String,
    
    /// Автомат, дающий сигналы
    automaton: Arc<A>,
    
    /// Состояние автомата
    state: A::State,
    
    /// Целевой параметр
    target: ParameterTarget,
    
    /// Наблюдатель (следит за нарушениями)
    observer: MicroControlObserver,
    
    /// Последнее значение
    last_value: f32,
}

impl<A: Automaton> WildServo<A> {
    pub fn new(
        name: impl Into<String>,
        automaton: Arc<A>,
        target: ParameterTarget,
        observer: MicroControlObserver,
    ) -> Self {
        let state = automaton.initial_state();
        
        Self {
            name: name.into(),
            automaton,
            state,
            target,
            observer,
            last_value: 0.0,
        }
    }
    
    /// Обновить значение (вызывается из audio-потока)
    pub fn update(&mut self, time: WorldTime, graph: &mut AudioGraph) {
        let (new_state, _) = self.automaton.step(
            time.absolute,
            &A::Action::default(),
            &self.state,
        );

        self.state = new_state;
        let raw_value = self.automaton.extract_value(&self.state);
        let value = self.target.scale(raw_value);
        
        // Если значение не изменилось, ничего не делаем
        if (value - self.last_value).abs() < 0.001 {
            return;
        }
        self.last_value = value;
        
        // Пытаемся применить значение напрямую (микро-контроль)
        let result = graph.with_parameter_observed(
            self.target.port,
            &self.target.parameter,
            &self.observer,
            &self.name,
            |node, param| {
                node.set_port_param(self.target.port, param, ParamValue::Float(value))
            },
        );
        
        if result.is_none() {
            log::warn!(
                "Серво {} не смогло применить значение {} к {}",
                self.name, value, self.target.port
            );
        }
    }
    
    /// Получить имя
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Обычное серво (безопасное, через очереди)
pub struct SafeServo<A: Automaton> {
    name: String,
    automaton: Arc<A>,
    state: A::State,
    target: ParameterTarget,
    cmd_queue: crossbeam_channel::Sender<rill_core::queues::Command>,
    last_value: f32,
}

impl<A: Automaton> SafeServo<A> {
    pub fn new(
        name: impl Into<String>,
        automaton: Arc<A>,
        target: ParameterTarget,
        cmd_queue: crossbeam_channel::Sender<rill_core::queues::Command>,
    ) -> Self {
        let state = automaton.initial_state();
        
        Self {
            name: name.into(),
            automaton,
            state,
            target,
            cmd_queue,
            last_value: 0.0,
        }
    }
    
    pub fn update(&mut self, time: WorldTime) {
        let (new_state, _) = self.automaton.step(
            time.absolute,
            &A::Action::default(),
            &self.state,
        );

        self.state = new_state;
        let raw_value = self.automaton.extract_value(&self.state);
        let value = self.target.scale(raw_value);
        
        if (value - self.last_value).abs() < 0.001 {
            return;
        }
        self.last_value = value;
        
        // Отправляем команду через очередь (безопасно)
        let cmd = rill_core::queues::SetParameter::new(
            self.target.port,
            self.target.parameter.clone(),
            value,
            format!("servo:{}", self.name),
        );
        
        let _ = self.cmd_queue.send(rill_core::queues::CommandEnum::SetParameter(cmd));
    }
}