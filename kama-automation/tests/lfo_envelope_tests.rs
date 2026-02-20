use std::sync::Arc;
use kama_core_traits::time::{SystemClock, Clock, TimeProvider, TickInfo};  // <-- Обновляем импорт
use kama_automation::{
    AutomationContext, LfoAction, Automaton,
    automaton::{LfoWithEnvelopeAutomaton, LfoWithEnvelopeState, EnvelopeStage},
};

// Вспомогательная структура для тестового TimeProvider
#[derive(Debug, Clone)]
struct TestTimeProvider {
    clock: Arc<SystemClock>,
}

impl TestTimeProvider {
    fn new() -> Self {
        Self {
            clock: Arc::new(SystemClock::new(44100.0, 120.0)),
        }
    }
    
    fn advance(&self, samples: u64) {
        self.clock.advance(samples);
    }
}

impl Clock for TestTimeProvider {
    fn sample_rate(&self) -> f64 {
        self.clock.sample_rate()
    }
    
    fn position_samples(&self) -> u64 {
        self.clock.position_samples()
    }
    
    fn advance(&self, samples: u64) -> u64 {
        self.clock.advance(samples)
    }
    
    fn reset(&self) {
        self.clock.reset()
    }
}

impl TimeProvider for TestTimeProvider {
    fn bpm(&self) -> f64 {
        self.clock.bpm()
    }
    
    fn set_bpm(&self, bpm: f64) {
        self.clock.set_bpm(bpm)
    }
    
    fn tick_info(&self) -> TickInfo {
        self.clock.tick_info()
    }
}

#[test]
fn test_lfo_with_envelope_initial_state() {
    println!("\n=== test_lfo_with_envelope_initial_state ===");
    
    let time_provider = Arc::new(TestTimeProvider::new());
    let context = AutomationContext::new(time_provider.clone());
    
    let lfo = LfoWithEnvelopeAutomaton::new(0.5, 0.8, 0.5, 1.0, 1.0);
    
    let state = lfo.initial_state();
    println!("Initial state - phase: {:.6}", state.phase);
    println!("Initial envelope - stage: {:?}, value: {:.6}", 
             state.envelope_state.stage, state.envelope_state.value);
    
    assert_eq!(state.phase, 0.0);
    assert_eq!(state.envelope_state.stage, EnvelopeStage::Off);
    assert_eq!(state.envelope_state.value, 0.0);
    assert_eq!(state.envelope_state.samples_elapsed, 0);
}

#[test]
fn test_lfo_with_envelope_trigger() {
    println!("\n=== test_lfo_with_envelope_trigger ===");
    
    let time_provider = Arc::new(TestTimeProvider::new());
    let context = AutomationContext::new(time_provider.clone());
    
    let lfo = LfoWithEnvelopeAutomaton::new(0.5, 0.8, 0.5, 2.0, 2.0);
    let mut state = lfo.initial_state();
    
    println!("Initial state - envelope: {:?}, value: {:.6}", 
             state.envelope_state.stage, state.envelope_state.value);
    
    let (new_state, _) = lfo.step(0.0, &context, LfoAction::Trigger, &state);
    state = new_state;
    
    println!("After trigger - envelope: {:?}, value: {:.6}", 
             state.envelope_state.stage, state.envelope_state.value);
    
    assert_eq!(state.envelope_state.stage, EnvelopeStage::Attack);
    assert!(state.envelope_state.value >= 0.0 && state.envelope_state.value <= 1.0);
}

#[test]
fn test_lfo_with_envelope_full_cycle() {
    println!("\n=== test_lfo_with_envelope_full_cycle ===");
    
    let time_provider = Arc::new(TestTimeProvider::new());
    let context = AutomationContext::new(time_provider.clone());
    
    let lfo = LfoWithEnvelopeAutomaton::new(0.5, 0.8, 0.5, 1.0, 1.0);
    let mut state = lfo.initial_state();
    
    println!("Time(s)\tPhase\tStage\t\tEnvVal\tOutput");
    println!("------\t-----\t-----\t\t------\t------");
    
    let (new_state, _) = lfo.step(0.0, &context, LfoAction::Trigger, &state);
    state = new_state;
    
    let mut outputs = Vec::new();
    
    for i in 0..=10 {
        let t = i as f64 * 0.5;
        if i > 0 {
            time_provider.advance(22050);
            let (new_state, _) = lfo.step(t, &context, LfoAction::None, &state);
            state = new_state;
        }
        
        let output = lfo.extract_value(&state);
        outputs.push(output);
        println!("{:.1}\t{:.3}\t{:?}\t{:.3}\t{:.3}", 
                 t, state.phase, state.envelope_state.stage, 
                 state.envelope_state.value, output);
    }
    
    let mut changed = false;
    for i in 1..outputs.len() {
        if (outputs[i] - outputs[i-1]).abs() > 0.01 {
            changed = true;
            break;
        }
    }
    assert!(changed, "Values should change over time: {:?}", outputs);
}

#[test]
fn test_lfo_with_envelope_parameter_updates() {
    println!("\n=== test_lfo_with_envelope_parameter_updates ===");
    
    let time_provider = Arc::new(TestTimeProvider::new());
    let context = AutomationContext::new(time_provider.clone());
    
    let lfo = LfoWithEnvelopeAutomaton::new(0.5, 0.8, 0.5, 1.0, 1.0);
    let mut state = lfo.initial_state();
    
    let (new_state, _) = lfo.step(0.0, &context, LfoAction::Trigger, &state);
    state = new_state;
    
    time_provider.advance(22050);
    let (new_state, _) = lfo.step(0.5, &context, LfoAction::None, &state);
    state = new_state;
    
    let value_before = lfo.extract_value(&state);
    println!("Value before frequency change: {:.6}", value_before);
    
    let (new_state, _) = lfo.step(0.5, &context, LfoAction::SetFrequency(1.0), &state);
    state = new_state;
    
    time_provider.advance(22050);
    let (new_state, _) = lfo.step(1.0, &context, LfoAction::None, &state);
    state = new_state;
    
    let value_after = lfo.extract_value(&state);
    println!("Value after frequency change: {:.6}", value_after);
    
    assert!((value_before - value_after).abs() > 0.01,
            "Values should differ: {} vs {}", value_before, value_after);
}

#[test]
fn test_lfo_with_envelope_attack_progression() {
    println!("\n=== test_lfo_with_envelope_attack_progression ===");
    
    let time_provider = Arc::new(TestTimeProvider::new());
    let context = AutomationContext::new(time_provider.clone());
    
    let lfo = LfoWithEnvelopeAutomaton::new(0.5, 0.8, 0.5, 2.0, 2.0);
    let mut state = lfo.initial_state();
    
    // Trigger at t=0
    let (new_state, _) = lfo.step(0.0, &context, LfoAction::Trigger, &state);
    state = new_state;
    println!("t=0.0s: envelope={:.3}, stage={:?}, last_time={}", 
             state.envelope_state.value, state.envelope_state.stage, state.last_time);
    
    // Advance time and step at t=0.5
    time_provider.advance(22050);
    let (new_state, _) = lfo.step(0.5, &context, LfoAction::None, &state);
    state = new_state;
    println!("t=0.5s: envelope={:.3}, stage={:?}, last_time={}", 
             state.envelope_state.value, state.envelope_state.stage, state.last_time);
    
    assert_eq!(state.envelope_state.stage, EnvelopeStage::Attack);
    assert!((state.envelope_state.value - 0.25).abs() < 0.1, 
            "Expected ~0.25, got {}", state.envelope_state.value);
    
    // Advance time and step at t=1.5
    time_provider.advance(44100);
    let (new_state, _) = lfo.step(1.5, &context, LfoAction::None, &state);
    state = new_state;
    println!("t=1.5s: envelope={:.3}, stage={:?}, last_time={}", 
             state.envelope_state.value, state.envelope_state.stage, state.last_time);
    
    assert_eq!(state.envelope_state.stage, EnvelopeStage::Attack);
    assert!((state.envelope_state.value - 0.75).abs() < 0.1,
            "Expected ~0.75, got {}", state.envelope_state.value);
    
    // Advance time and step at t=2.0
    time_provider.advance(22050);
    let (new_state, _) = lfo.step(2.0, &context, LfoAction::None, &state);
    state = new_state;
    println!("t=2.0s: envelope={:.3}, stage={:?}, last_time={}", 
             state.envelope_state.value, state.envelope_state.stage, state.last_time);
    
    assert!(state.envelope_state.stage == EnvelopeStage::Decay || 
            state.envelope_state.stage == EnvelopeStage::Sustain,
            "Expected Decay or Sustain, got {:?}", state.envelope_state.stage);
    assert!((state.envelope_state.value - 1.0).abs() < 0.1,
            "Expected ~1.0, got {}", state.envelope_state.value);
}

#[test]
fn test_lfo_with_envelope_release() {
    println!("\n=== test_lfo_with_envelope_release ===");
    
    let time_provider = Arc::new(TestTimeProvider::new());
    let context = AutomationContext::new(time_provider.clone());
    
    let lfo = LfoWithEnvelopeAutomaton::new(0.5, 0.8, 0.5, 1.0, 2.0);
    let mut state = lfo.initial_state();
    
    // Trigger at t=0
    let (new_state, _) = lfo.step(0.0, &context, LfoAction::Trigger, &state);
    state = new_state;
    println!("t=0.0s: envelope={:.3}, stage={:?}", 
             state.envelope_state.value, state.envelope_state.stage);
    
    // Advance to t=1.0 (attack complete)
    time_provider.advance(44100);
    let (new_state, _) = lfo.step(1.0, &context, LfoAction::None, &state);
    state = new_state;
    println!("t=1.0s: envelope={:.3}, stage={:?}", 
             state.envelope_state.value, state.envelope_state.stage);
    
    // Check that we're out of Attack stage
    assert!(state.envelope_state.stage != EnvelopeStage::Attack,
            "Should not be in Attack after 1s");
    assert!(state.envelope_state.value > 0.9, 
            "Expected value near 1.0, got {}", state.envelope_state.value);
    
    // Advance to t=2.0 (should be in Sustain)
    time_provider.advance(44100);
    let (new_state, _) = lfo.step(2.0, &context, LfoAction::None, &state);
    state = new_state;
    println!("t=2.0s: envelope={:.3}, stage={:?}", 
             state.envelope_state.value, state.envelope_state.stage);
    
    assert!(state.envelope_state.stage == EnvelopeStage::Sustain,
            "Expected Sustain, got {:?}", state.envelope_state.stage);
    assert!((state.envelope_state.value - 1.0).abs() < 0.1,
            "Expected value near 1.0, got {}", state.envelope_state.value);
    
    println!("Note: Full release test requires separate Release action");
}