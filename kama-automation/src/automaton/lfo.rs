// kama-automation/src/automaton/lfo.rs

use super::{Automaton, AutomationContext, EnvelopeState, EnvelopeStage};

/// Действия LFO
#[derive(Debug, Clone, Default, PartialEq)]
pub enum LfoAction {
    #[default]
    None,
    SetFrequency(f64),
    SetAmplitude(f64),
    Trigger,
}

/// Состояние LFO
#[derive(Debug, Clone, Default)]
pub struct LfoState {
    pub phase: f64,
    pub last_time: f64,
    // Убираем envelope_state из основной структуры
}

/// Состояние LFO с envelope (для случаев, когда он нужен)
#[derive(Debug, Clone)]
pub struct LfoWithEnvelopeState {
    pub phase: f64,
    pub last_time: f64,
    pub envelope_state: EnvelopeState,
}

/// LFO автомат
pub struct LfoAutomaton {
    pub frequency: f64,
    pub amplitude: f64,
    pub offset: f64,
    pub attack_time: f64,
    pub release_time: f64,
    pub use_envelope: bool,
}

impl LfoAutomaton {
    pub fn new(frequency: f64, amplitude: f64, offset: f64) -> Self {
        Self {
            frequency,
            amplitude,
            offset,
            attack_time: 0.01,
            release_time: 0.01,
            use_envelope: false,
        }
    }
    
    pub fn with_envelope(mut self, attack: f64, release: f64) -> Self {
        self.attack_time = attack;
        self.release_time = release;
        self.use_envelope = true;
        self
    }
    
    fn update_envelope(&self, envelope: &mut EnvelopeState, time_delta: f64, sample_rate: f64) {
        let samples_delta = (time_delta * sample_rate) as usize;
        
        match envelope.stage {
            EnvelopeStage::Attack => {
                let attack_samples = (self.attack_time * sample_rate) as usize;
                envelope.samples_elapsed += samples_delta;
                
                if envelope.samples_elapsed >= attack_samples {
                    envelope.stage = EnvelopeStage::Decay;
                    envelope.value = 1.0;
                    envelope.samples_elapsed = 0;
                } else {
                    envelope.value = envelope.samples_elapsed as f64 / attack_samples as f64;
                }
            }
            
            EnvelopeStage::Decay => {
                envelope.stage = EnvelopeStage::Sustain;
                envelope.value = 1.0;
                envelope.samples_elapsed = 0;
            }
            
            EnvelopeStage::Sustain => {
                envelope.value = 1.0;
            }
            
            EnvelopeStage::Release => {
                let release_samples = (self.release_time * sample_rate) as usize;
                envelope.samples_elapsed += samples_delta;
                
                if envelope.samples_elapsed >= release_samples {
                    envelope.stage = EnvelopeStage::Off;
                    envelope.value = 0.0;
                    envelope.samples_elapsed = 0;
                } else {
                    envelope.value = 1.0 - (envelope.samples_elapsed as f64 / release_samples as f64);
                }
            }
            
            EnvelopeStage::Off => {
                envelope.value = 0.0;
            }
        }
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
        
        let time_delta = if state.last_time > 0.0 {
            time - state.last_time
        } else {
            0.0
        };
        
        println!("LfoAutomaton - time: {:.6}, last_time: {:.6}, delta: {:.6}", 
                 time, state.last_time, time_delta);
        
        match action {
            LfoAction::SetFrequency(freq) => {
                println!("LfoAutomaton - setting frequency to {}", freq);
            }
            LfoAction::SetAmplitude(amp) => {
                println!("LfoAutomaton - setting amplitude to {}", amp);
            }
            LfoAction::Trigger => {
                println!("LfoAutomaton - trigger");
                // При trigger ничего не делаем, так как у нас нет envelope
            }
            LfoAction::None => {}
        }
        
        if time_delta > 0.0 {
            new_state.phase += self.frequency * time_delta;
            while new_state.phase >= 1.0 {
                new_state.phase -= 1.0;
            }
        }
        
        new_state.last_time = time;
        
        println!("LfoAutomaton - new phase: {:.6}", new_state.phase);
        
        (new_state, next_action)
    }
    
    fn initial_state(&self) -> LfoState {
        LfoState {
            phase: 0.0,
            last_time: 0.0,
        }
    }
    
    fn name(&self) -> &str {
        "LFO"
    }
    
    fn extract_value(&self, state: &LfoState) -> f64 {
        println!("extract_value - phase: {:.6}", state.phase);
        
        let sin_val = (state.phase * 2.0 * std::f64::consts::PI).sin();
        println!("extract_value - sin_val: {:.6}", sin_val);
        
        let result = sin_val * self.amplitude + self.offset;
        println!("extract_value - result: {:.6}", result);
        
        result
    }
}

// Отдельный тип для LFO с envelope
pub struct LfoWithEnvelopeAutomaton {
    lfo: LfoAutomaton,
}

impl LfoWithEnvelopeAutomaton {
    pub fn new(frequency: f64, amplitude: f64, offset: f64, attack: f64, release: f64) -> Self {
        Self {
            lfo: LfoAutomaton::new(frequency, amplitude, offset)
                .with_envelope(attack, release),
        }
    }
}

impl Automaton for LfoWithEnvelopeAutomaton {
    type Time = f64;
    type Context = AutomationContext;
    type Action = LfoAction;
    type State = LfoWithEnvelopeState;
    
    fn step(
        &self,
        time: f64,
        context: &AutomationContext,
        action: LfoAction,
        state: &LfoWithEnvelopeState,
    ) -> (LfoWithEnvelopeState, Option<LfoAction>) {
        let mut new_state = state.clone();
        let next_action = None;
        
        // Вычисляем прошедшее время с последнего шага
        let time_delta = if state.last_time > 0.0 {
            time - state.last_time
        } else {
            // Если это первый шаг, используем время как дельту
            time
        };
        
        println!("LfoWithEnvelope - time: {:.6}, last_time: {:.6}, delta: {:.6}", 
                 time, state.last_time, time_delta);
        
        match action {
            LfoAction::SetFrequency(freq) => {
                println!("LfoWithEnvelope - setting frequency to {}", freq);
            }
            LfoAction::SetAmplitude(amp) => {
                println!("LfoWithEnvelope - setting amplitude to {}", amp);
            }
            LfoAction::Trigger => {
                println!("LfoWithEnvelope - trigger");
                new_state.envelope_state = EnvelopeState {
                    stage: EnvelopeStage::Attack,
                    value: 0.0,
                    samples_elapsed: 0,
                };
            }
            LfoAction::None => {}
        }
        
        // Всегда обновляем фазу на основе времени
        if time_delta > 0.0 {
            new_state.phase += self.lfo.frequency * time_delta;
            while new_state.phase >= 1.0 {
                new_state.phase -= 1.0;
            }
        }
        
        // Всегда обновляем envelope, если он есть
        if new_state.envelope_state.stage != EnvelopeStage::Off {
            let sample_rate = context.time.sample_rate();
            self.lfo.update_envelope(&mut new_state.envelope_state, time_delta, sample_rate);
        }
        
        new_state.last_time = time;
        
        println!("LfoWithEnvelope - new phase: {:.6}, envelope: {:?}, value: {:.6}, last_time: {:.6}", 
                 new_state.phase, new_state.envelope_state.stage, 
                 new_state.envelope_state.value, new_state.last_time);
        
        (new_state, next_action)
    }
    
    fn initial_state(&self) -> LfoWithEnvelopeState {
        LfoWithEnvelopeState {
            phase: 0.0,
            last_time: 0.0,
            envelope_state: EnvelopeState {
                stage: EnvelopeStage::Off,
                value: 0.0,
                samples_elapsed: 0,
            },
        }
    }
    
    fn name(&self) -> &str {
        "LFO+Envelope"
    }
    
    fn extract_value(&self, state: &LfoWithEnvelopeState) -> f64 {
        println!("extract_value - phase: {:.6}, envelope: {:.6}", 
                 state.phase, state.envelope_state.value);
        
        let sin_val = (state.phase * 2.0 * std::f64::consts::PI).sin();
        println!("extract_value - sin_val: {:.6}", sin_val);
        
        let result = sin_val * self.lfo.amplitude * state.envelope_state.value + self.lfo.offset;
        println!("extract_value - result: {:.6}", result);
        
        result
    }
}