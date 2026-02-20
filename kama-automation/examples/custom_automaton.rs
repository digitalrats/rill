//! Пример создания собственного автомата
//!
//! Запуск: cargo run --example custom_automaton

use std::sync::Arc;
use std::time::Duration;
use std::thread;
use kama_core_traits::time::{SystemClock, Clock};  // Добавили импорт Clock
use kama_automation::{
    AutomationContext, AutomationManager, Automaton,
    Servo, ParameterMapping, TestSignalSender,
};

#[derive(Debug, Clone)]
struct RandomWalkState {
    value: f64,
    last_time: f64,
}

struct RandomWalkAutomaton {
    min: f64,
    max: f64,
    step_size: f64,
    change_rate: f64,
}

impl RandomWalkAutomaton {
    fn new(min: f64, max: f64, step_size: f64, change_rate: f64) -> Self {
        Self { min, max, step_size, change_rate }
    }
}

impl Automaton for RandomWalkAutomaton {
    type Time = f64;
    type Context = AutomationContext;
    type Action = ();
    type State = RandomWalkState;
    
    fn step(
        &self,
        time: f64,
        _context: &Self::Context,
        _action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<Self::Action>) {
        let mut new_state = state.clone();
        
        let time_delta = if state.last_time > 0.0 {
            time - state.last_time
        } else {
            0.0
        };
        
        let expected_changes = time_delta * self.change_rate;
        let actual_changes = expected_changes.floor() as usize;
        
        for _ in 0..actual_changes {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            let step = if rng.gen_bool(0.5) { self.step_size } else { -self.step_size };
            new_state.value = (new_state.value + step).clamp(self.min, self.max);
        }
        
        new_state.last_time = time;
        (new_state, None)
    }
    
    fn initial_state(&self) -> Self::State {
        RandomWalkState {
            value: (self.min + self.max) / 2.0,
            last_time: 0.0,
        }
    }
    
    fn name(&self) -> &str {
        "RandomWalk"
    }
    
    fn extract_value(&self, state: &Self::State) -> f64 {
        state.value
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Custom Automaton Example (Random Walk) ===\n");

    let clock = Arc::new(SystemClock::new(44100.0, 120.0));
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());
    
    let mut manager = AutomationManager::new(clock.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());
    
    println!("Создаём Random Walk автомат...");
    
    let automaton = Arc::new(RandomWalkAutomaton::new(
        0.0, 1.0, 0.05, 2.0
    ));
    
    let context = AutomationContext::new(clock.clone());
    
    let servo = Servo::new(
        "random_walk".to_string(),
        automaton,
        "effect".to_string(),
        "parameter".to_string(),
        ParameterMapping::Linear,
        context,
    );
    
    manager.add_servo(servo);
    
    println!("Автомат добавлен\n");
    println!("Время(s)\tЗначение");
    println!("--------\t--------");
    
    for i in 0..50 {
        let time = i as f64 * 0.1;
        
        Clock::advance(clock.as_ref(), 4410);
        manager.update(4410);
        
        let signals = signal_sender.get_signals_for_param("effect", "parameter");
        if let Some(&value) = signals.last() {
            println!("{:.1}\t\t{:.3}", time, value);
        }
        
        thread::sleep(Duration::from_millis(20));
    }
    
    println!("\n✅ Свой автомат успешно работает");
    Ok(())
}