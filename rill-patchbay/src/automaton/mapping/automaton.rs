//! Автомат маппинга — преобразует сигналы от сенсоров

use super::core::{MappingRule, Transform};
use crate::core::{AutomatonContext, SignalOrigin, SignalValue, WorldSignal};
use crate::automaton::{Automaton, ProcessorAutomaton};
use std::collections::HashMap;

/// Автомат маппинга — преобразует сигналы от сенсоров
///
/// Работает в мире автоматов, получает сигналы через контекст
/// и генерирует новые сигналы для серво или других автоматов.
pub struct MappingAutomaton {
    /// Имя автомата
    name: String,
    
    /// Правила маппинга
    rules: Vec<MappingRule>,
    
    /// Последние входные значения (для гистерезиса)
    last_inputs: HashMap<String, f32>,
    
    /// Последние выходные значения (для гистерезиса)
    last_outputs: HashMap<String, f32>,
    
    /// Порог гистерезиса (0.0-0.1)
    threshold: f32,
}

impl MappingAutomaton {
    /// Создать новый автомат маппинга
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            rules: Vec::new(),
            last_inputs: HashMap::new(),
            last_outputs: HashMap::new(),
            threshold: 0.01, // 1% по умолчанию
        }
    }
    
    /// Добавить правило
    pub fn add_rule(&mut self, rule: MappingRule) {
        self.rules.push(rule);
    }
    
    /// Добавить правило (builder style)
    pub fn with_rule(mut self, rule: MappingRule) -> Self {
        self.rules.push(rule);
        self
    }
    
    /// Установить порог гистерезиса
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold.clamp(0.0, 0.1);
        self
    }
    
    /// Получить правило по имени входа
    fn find_rules_by_input(&self, input_name: &str) -> Vec<&MappingRule> {
        self.rules.iter()
            .filter(|r| r.input_name == input_name)
            .collect()
    }
}

impl Automaton for MappingAutomaton {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn process(&mut self, context: &AutomatonContext) -> Vec<WorldSignal> {
        let mut outputs = Vec::new();
        
        // Группируем входящие сигналы по имени
        let mut signals_by_name: HashMap<String, Vec<&WorldSignal>> = HashMap::new();
        for signal in &context.inputs {
            let name = signal.origin.to_string();
            signals_by_name.entry(name).or_default().push(signal);
        }
        
        // Для каждого правила ищем соответствующий входной сигнал
        for rule in &self.rules {
            if let Some(signals) = signals_by_name.get(&rule.input_name) {
                // Берем последний сигнал (или можно усреднять)
                if let Some(signal) = signals.last() {
                    let input_val = signal.value.normalized;
                    let last_input = self.last_inputs.get(&rule.input_name).copied().unwrap_or(-1.0);
                    
                    // Если значение изменилось значимо, применяем правило
                    if (input_val - last_input).abs() > self.threshold {
                        self.last_inputs.insert(rule.input_name.clone(), input_val);
                        
                        let output_val = rule.apply(input_val);
                        let last_output = self.last_outputs.get(&rule.output_name).copied().unwrap_or(-1.0);
                        
                        // Отправляем только если выход тоже изменился
                        if (output_val - last_output).abs() > self.threshold {
                            self.last_outputs.insert(rule.output_name.clone(), output_val);
                            
                            outputs.push(WorldSignal::new(
                                SignalOrigin::Automaton(format!("{}/{}", self.name, rule.output_name)),
                                SignalValue::continuous(output_val),
                            ));
                        }
                        
                        // Если указан целевой параметр, отправляем команду в граф
                        if let (Some(port), Some(param)) = (&rule.target_port, &rule.target_parameter) {
                            let cmd = rill_core::queues::SetParameter::new(
                                *port,
                                param.clone(),
                                output_val,
                                rill_core::queues::SignalSource::Automaton(self.name.clone()),
                            );
                            let _ = context.command_tx.send(rill_core::queues::CommandEnum::SetParameter(cmd));
                        }
                        
                        // Отправляем телеметрию
                        let _ = context.telemetry_tx.send(
                            rill_core::queues::Telemetry::event(
                                self.name.clone(),
                                "mapping",
                                vec![input_val, output_val],
                            )
                        );
                    }
                }
            }
        }
        
        outputs
    }
    
    fn peek(&self) -> f32 {
        // Возвращаем последнее выходное значение первого правила
        self.last_outputs.values().next().copied().unwrap_or(0.0)
    }
    
    fn reset(&mut self) {
        self.last_inputs.clear();
        self.last_outputs.clear();
    }
}

impl ProcessorAutomaton for MappingAutomaton {
    fn num_inputs(&self) -> usize {
        self.rules.len()
    }
    
    fn input(&self, idx: usize) -> Option<f32> {
        if idx < self.rules.len() {
            self.last_inputs.get(&self.rules[idx].input_name).copied()
        } else {
            None
        }
    }
}