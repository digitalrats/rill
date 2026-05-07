//! Mapping automaton — transforms signals from sensors

use super::core::{MappingRule, Transform};
use crate::core::{AutomatonContext, SignalOrigin, SignalValue, WorldSignal};
use crate::automaton::{Automaton, ProcessorAutomaton};
use std::collections::HashMap;

/// Mapping automaton — transforms signals from sensors
///
/// Operates in the automaton world, receives signals through the context
/// and generates new signals for servos or other automata.
pub struct MappingAutomaton {
    /// Automaton name
    name: String,
    
    /// Mapping rules
    rules: Vec<MappingRule>,
    
    /// Last input values (for hysteresis)
    last_inputs: HashMap<String, f32>,
    
    /// Last output values (for hysteresis)
    last_outputs: HashMap<String, f32>,
    
    /// Hysteresis threshold (0.0-0.1)
    threshold: f32,
}

impl MappingAutomaton {
    /// Create a new mapping automaton
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            rules: Vec::new(),
            last_inputs: HashMap::new(),
            last_outputs: HashMap::new(),
            threshold: 0.01, // 1% default
        }
    }
    
    /// Add a rule
    pub fn add_rule(&mut self, rule: MappingRule) {
        self.rules.push(rule);
    }
    
    /// Add a rule (builder style)
    pub fn with_rule(mut self, rule: MappingRule) -> Self {
        self.rules.push(rule);
        self
    }
    
    /// Set the hysteresis threshold
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold.clamp(0.0, 0.1);
        self
    }
    
    /// Get rule by input name
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
        
        // Group incoming signals by name
        let mut signals_by_name: HashMap<String, Vec<&WorldSignal>> = HashMap::new();
        for signal in &context.inputs {
            let name = signal.origin.to_string();
            signals_by_name.entry(name).or_default().push(signal);
        }
        
        // For each rule, find the corresponding input signal
        for rule in &self.rules {
            if let Some(signals) = signals_by_name.get(&rule.input_name) {
                // Take the latest signal (or could average)
                if let Some(signal) = signals.last() {
                    let input_val = signal.value.normalized;
                    let last_input = self.last_inputs.get(&rule.input_name).copied().unwrap_or(-1.0);
                    
                    // If the value changed significantly, apply the rule
                    if (input_val - last_input).abs() > self.threshold {
                        self.last_inputs.insert(rule.input_name.clone(), input_val);
                        
                        let output_val = rule.apply(input_val);
                        let last_output = self.last_outputs.get(&rule.output_name).copied().unwrap_or(-1.0);
                        
                        // Only send if the output also changed
                        if (output_val - last_output).abs() > self.threshold {
                            self.last_outputs.insert(rule.output_name.clone(), output_val);
                            
                            outputs.push(WorldSignal::new(
                                SignalOrigin::Automaton(format!("{}/{}", self.name, rule.output_name)),
                                SignalValue::continuous(output_val),
                            ));
                        }
                        
                        // If a target parameter is specified, send a command to the graph
                        if let (Some(port), Some(param)) = (&rule.target_port, &rule.target_parameter) {
                            let cmd = rill_core::queues::SetParameter::new(
                                *port,
                                param.clone(),
                                rill_core::traits::ParamValue::Float(output_val),
                                rill_core::queues::SignalOrigin::Automaton(self.name.clone()),
                            );
                            let _ = context.command_tx.send(rill_core::queues::CommandEnum::SetParameter(cmd));
                        }
                        
                        // Send telemetry
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
        // Return the last output value of the first rule
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