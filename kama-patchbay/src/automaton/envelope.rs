//! Envelope автомат - генерирует огибающие

use crate::core::{AutomatonContext, WorldSignal, SignalOrigin};
use crate::automaton::{Automaton, ProcessorAutomaton};

/// Стадии огибающей
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Envelope автомат (ADSR)
pub struct EnvelopeAutomaton {
    name: String,
    attack: f32,
    decay: f32,
    sustain: f32,
    release: f32,
    
    stage: EnvelopeStage,
    level: f32,
    trigger: bool,
    sample_rate: f32,
    
    // Для детекции фронтов
    last_gate: bool,
}

impl EnvelopeAutomaton {
    pub fn new(name: impl Into<String>, sample_rate: f32) -> Self {
        Self {
            name: name.into(),
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.2,
            stage: EnvelopeStage::Idle,
            level: 0.0,
            trigger: false,
            sample_rate,
            last_gate: false,
        }
    }
    
    pub fn with_adsr(mut self, a: f32, d: f32, s: f32, r: f32) -> Self {
        self.attack = a.max(0.001);
        self.decay = d.max(0.001);
        self.sustain = s.clamp(0.0, 1.0);
        self.release = r.max(0.001);
        self
    }
    
    fn update_stage(&mut self, gate: bool, delta: f32) {
        match self.stage {
            EnvelopeStage::Idle => {
                if gate && !self.last_gate {
                    self.stage = EnvelopeStage::Attack;
                }
            }
            EnvelopeStage::Attack => {
                self.level += delta / self.attack;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvelopeStage::Decay;
                }
            }
            EnvelopeStage::Decay => {
                self.level -= delta / self.decay * (1.0 - self.sustain);
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    self.stage = EnvelopeStage::Sustain;
                }
            }
            EnvelopeStage::Sustain => {
                if !gate {
                    self.stage = EnvelopeStage::Release;
                }
            }
            EnvelopeStage::Release => {
                self.level -= delta / self.release * self.sustain;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = EnvelopeStage::Idle;
                }
            }
        }
    }
}

impl Automaton for EnvelopeAutomaton {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn process(&mut self, context: &AutomatonContext) -> Vec<WorldSignal> {
        // Ищем gate сигнал во входах
        let gate = context.inputs.iter()
            .find(|s| s.origin.to_string().contains("gate"))
            .map(|s| s.value > 0.5)
            .unwrap_or(false);
        
        self.update_stage(gate, context.time.delta as f32);
        self.last_gate = gate;
        
        vec![WorldSignal::new(
            SignalOrigin::Automaton(self.name.clone()),
            self.level,
        )]
    }
    
    fn peek(&self) -> f32 {
        self.level
    }
    
    fn reset(&mut self) {
        self.stage = EnvelopeStage::Idle;
        self.level = 0.0;
        self.last_gate = false;
    }
}

impl ProcessorAutomaton for EnvelopeAutomaton {
    fn num_inputs(&self) -> usize {
        1
    }
    
    fn input(&self, _idx: usize) -> Option<f32> {
        Some(if self.last_gate { 1.0 } else { 0.0 })
    }
}