use rill_automation::{
    automaton::{LfoAutomaton, LfoWithEnvelopeAutomaton},
    AutomationContext, AutomationManager, Automaton, ParameterMapping, Servo, TestSignalSender,
};
use rill_core::time::{Clock, SystemClock, TickInfo, TimeProvider};
use std::sync::Arc;

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
fn test_lfo_automaton_in_manager() {
    println!("\n=== test_lfo_automaton_in_manager ===");

    let time_provider = Arc::new(TestTimeProvider::new());
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());

    let mut manager = AutomationManager::new(time_provider.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());

    manager.add_lfo("test_lfo", 1.0, 0.5, 0.0, "node", "param");

    assert_eq!(manager.servos().len(), 1);

    // Обновляем несколько раз
    for _ in 0..10 {
        time_provider.advance(4410);
        manager.update(4410);
    }

    let signals = signal_sender.get_signals_for_param("node", "param");
    assert!(!signals.is_empty(), "Should have sent signals");
}

#[test]
fn test_lfo_with_envelope_in_manager() {
    println!("\n=== test_lfo_with_envelope_in_manager ===");

    let time_provider = Arc::new(TestTimeProvider::new());
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());

    let mut manager = AutomationManager::new(time_provider.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());

    manager.add_lfo_with_envelope("test_envelope", 1.0, 0.5, 0.0, 0.1, 0.2, "node", "param");

    assert_eq!(manager.servos().len(), 1);

    // Обновляем несколько раз
    for _ in 0..10 {
        time_provider.advance(4410);
        manager.update(4410);
    }

    let signals = signal_sender.get_signals_for_param("node", "param");
    assert!(!signals.is_empty(), "Should have sent signals");
}
