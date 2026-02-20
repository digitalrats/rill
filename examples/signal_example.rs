//! Пример использования сигнальной системы

use kama_signal::{SignalDispatcher, ParameterChanged, SignalSource};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Kama Core Signal Example ===");
    
    let dispatcher = SignalDispatcher::new();
    
    // Создаём тестовый сигнал
    let signal = ParameterChanged {
        node_id: "test_node".to_string(),
        parameter_id: "gain".to_string(),
        value: 0.75,
        normalized_value: 0.75,
        timestamp: 1234567890,
        source: SignalSource::UserInterface,
    };
    
    println!("Created signal: {:?}", signal);
    println!("Signal type id: {:?}", signal.type_id());
    
    println!("Signal system is ready!");
    Ok(())
}
