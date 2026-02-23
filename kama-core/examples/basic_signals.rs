//! Базовый пример использования сигнальной системы

use kama_core::signal::*;
use kama_core::traits::{NodeId, ParameterId, PortId};

fn main() {
    println!("=== Basic Signals Example ===");
    
    // Создаём шину для сигналов изменения параметров
    let bus = SignalBus::<ParameterChanged>::new(BusConfig::Unbounded);
    let receiver = bus.receiver();
    
    // Создаём идентификаторы
    let node = NodeId(42);
    let port = PortId::node(node);
    let param = ParameterId::new("frequency").unwrap();
    
    // Создаём и отправляем сигнал
    let signal = ParameterChanged::new(
        port,
        param.clone(),
        440.0,
        0.5,
        SignalSource::Automation,
    );
    
    println!("Отправляем сигнал: порт={}, параметр={}, значение={}",
             signal.port, signal.parameter, signal.value);
    bus.send(signal).unwrap();
    
    // Получаем сигнал
    match receiver.try_recv() {
        Ok(received) => {
            println!("Получен сигнал: порт={}, параметр={}, значение={}",
                     received.port, received.parameter, received.value);
        }
        Err(e) => {
            println!("Ошибка при получении сигнала: {:?}", e);
        }
    }
    
    println!("\n✅ Signals example completed!");
}