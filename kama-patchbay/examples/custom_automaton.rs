//! Пример создания собственного автомата через FunctionAutomaton

use kama_automation::{
    automaton::{FunctionAutomaton, StatefulFunctionAutomaton},
    AutomationContext,
    AutomationManager,
    Automaton,
    ParameterMapping,
    Servo,
    TestSignalSender,
};
use kama_core::traits::time::{Clock, SystemClock};
use kama_core::traits::{NodeId, ParameterId, PortId};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Custom Automaton Examples ===\n");

    let clock = Arc::new(SystemClock::new(44100.0, 120.0));
    let system_clock = SystemClock::new(44100.0, 120.0);
    let signal_sender = Arc::new(TestSignalSender::new());

    let mut manager = AutomationManager::new(clock.clone(), system_clock)
        .with_signal_sender(signal_sender.clone());

    let node = NodeId(1);
    let port = PortId::control_in(node, 0);

    // Пример 1: Простое замыкание
    println!("1. Simple closure:");
    let simple_param = ParameterId::new("volume")?;
    let simple_automaton = FunctionAutomaton::new(
        "Simple Sine",
        |t: f64| (t * 0.5).sin() * 0.3 + 0.5,
        simple_param.clone(),
    );

    let context = AutomationContext::new(clock.clone());
    let servo = Servo::new(
        "simple".to_string(),
        Arc::new(simple_automaton),
        port,
        simple_param,
        ParameterMapping::Linear,
        context,
    );
    manager.add_servo(servo);

    // Пример 2: Автомат с состоянием (интегратор)
    println!("\n2. Stateful automaton (integrator):");
    let integrator_param = ParameterId::new("position")?;

    let integrator = StatefulFunctionAutomaton::new(
        "Integrator",
        |_time: f64, state: &mut f64| {
            *state += 0.01;
            if *state > 1.0 {
                *state = 0.0;
            }
            *state
        },
        0.0,
        integrator_param.clone(),
    );

    let context = AutomationContext::new(clock.clone());
    let servo = Servo::new(
        "integrator".to_string(),
        Arc::new(integrator),
        port,
        integrator_param,
        ParameterMapping::Linear,
        context,
    );
    manager.add_servo(servo);

    // Пример 3: Random Walk
    println!("\n3. Random Walk:");
    let random_param = ParameterId::new("random_param")?;

    let random_walk = StatefulFunctionAutomaton::new(
        "Random Walk",
        |_time: f64, state: &mut f64| {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            let step = if rng.gen_bool(0.5) { 0.03 } else { -0.03 };
            *state = (*state + step).clamp(0.0, 1.0);
            *state
        },
        0.5,
        random_param.clone(),
    );

    let context = AutomationContext::new(clock.clone());
    let servo = Servo::new(
        "random".to_string(),
        Arc::new(random_walk),
        port,
        random_param,
        ParameterMapping::Linear,
        context,
    );
    manager.add_servo(servo);

    println!("\nЗапуск автоматизации...\n");
    println!("Время(s)\tSimple\t\tIntgr\t\tRandom");
    println!("--------\t------\t\t-----\t\t------");

    for i in 0..50 {
        let time = i as f64 * 0.1;

        clock.advance(4410);
        manager.update(4410);

        let simple = signal_sender
            .get_signals_for_param(&ParameterId::new("volume")?)
            .last()
            .map(|s| s.value)
            .unwrap_or(0.5);
        let integrator = signal_sender
            .get_signals_for_param(&ParameterId::new("position")?)
            .last()
            .map(|s| s.value)
            .unwrap_or(0.0);
        let random = signal_sender
            .get_signals_for_param(&ParameterId::new("random_param")?)
            .last()
            .map(|s| s.value)
            .unwrap_or(0.5);

        println!(
            "{:.1}\t\t{:.3}\t\t{:.3}\t\t{:.3}",
            time, simple, integrator, random
        );

        thread::sleep(Duration::from_millis(20));
    }

    println!("\n✅ Все кастомные автоматы работают");
    Ok(())
}