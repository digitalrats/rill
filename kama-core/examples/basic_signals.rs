//! Базовый пример использования сигнальной системы

use kama_core::signal::*;

fn main() {
    println!("=== Basic Signals Example ===");
    
    // Создаём шину для сигналов изменения параметров
    let bus = SignalBus::<ParameterChanged>::new(BusConfig::Unbounded);
    let receiver = bus.receiver();
    
    // Создаём и отправляем сигнал
    let signal = ParameterChanged {
        node_id: "node_42".to_string(),  // используем String
        parameter_id: "frequency".to_string(),
        value: 440.0,
        normalized_value: 0.5,
        timestamp: 12345,
        source: SignalSource::Automation,
    };
    
    println!("Отправляем сигнал: node_id={}, param={}, value={}", 
             signal.node_id, signal.parameter_id, signal.value);
    bus.send(signal).unwrap();
    
    // Получаем сигнал
    match receiver.try_recv() {
        Ok(received) => {
            println!("Получен сигнал: node_id={}, param={}, value={}", 
                     received.node_id, received.parameter_id, received.value);
        }
        Err(e) => {
            println!("Ошибка при получении сигнала: {:?}", e);
        }
    }
    
    println!("\n✅ Signals example completed!");
}