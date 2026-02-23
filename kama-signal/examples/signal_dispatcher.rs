//! Пример использования SimpleSignalDispatcher
//!
//! Запуск: cargo run --example signal_dispatcher

use kama_core::signal::{
    ClockTick, ParameterChanged, SignalHandler, SignalSource, SimpleSignalDispatcher, SystemEvent,
};
use std::sync::{Arc, Mutex};

// Обработчик для логирования - теперь с Clone
#[derive(Clone)]
struct LoggingHandler {
    name: String,
    log: Arc<Mutex<Vec<String>>>,
}

impl LoggingHandler {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            log: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn print_log(&self) {
        let log = self.log.lock().unwrap();
        println!("Лог обработчика '{}':", self.name);
        for entry in log.iter() {
            println!("  {}", entry);
        }
    }
}

impl SignalHandler<ParameterChanged> for LoggingHandler {
    fn handle(&mut self, signal: &ParameterChanged) {
        let msg = format!(
            "[{}] Параметр {}:{} = {:.2}",
            self.name, signal.node_id, signal.parameter_id, signal.value
        );
        println!("{}", msg);
        self.log.lock().unwrap().push(msg);
    }
}

impl SignalHandler<ClockTick> for LoggingHandler {
    fn handle(&mut self, signal: &ClockTick) {
        let msg = format!("[{}] Такт: позиция {}", self.name, signal.sample_pos);
        println!("{}", msg);
        self.log.lock().unwrap().push(msg);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Signal Dispatcher Example ===\n");

    let mut dispatcher = SimpleSignalDispatcher::new();

    // Создаём обработчики
    let handler1 = LoggingHandler::new("Handler1");
    let handler2 = LoggingHandler::new("Handler2");
    let handler3 = LoggingHandler::new("Handler3 (только параметры)");

    // Регистрируем обработчики - теперь можно клонировать
    dispatcher.register::<ParameterChanged, _>(handler1.clone());
    dispatcher.register::<ParameterChanged, _>(handler2.clone());
    dispatcher.register::<ParameterChanged, _>(handler3);
    dispatcher.register::<ClockTick, _>(handler1); // handler1 получает и такты
    dispatcher.register::<ClockTick, _>(handler2); // handler2 получает и такты

    println!("Отправляем сигналы...\n");

    // Отправляем ParameterChanged сигналы
    for i in 0..3 {
        let signal = ParameterChanged {
            node_id: "osc".to_string(),
            parameter_id: format!("freq_{}", i),
            value: 440.0 + i as f32 * 100.0,
            normalized_value: 0.5,
            timestamp: 1234567890 + i as u64,
            source: SignalSource::Automation,
        };

        dispatcher.emit(signal)?;
        println!("  Отправлен ParameterChanged #{}", i);
    }

    // Отправляем ClockTick сигналы
    for i in 0..2 {
        let signal = ClockTick {
            sample_pos: 44100 * (i + 1),
            samples_since_last: 44100,
        };

        dispatcher.emit(signal)?;
        println!("  Отправлен ClockTick #{}", i);
    }

    // Пытаемся отправить сигнал незарегистрированного типа
    let event = SystemEvent::GraphChanged;
    match dispatcher.emit(event) {
        Ok(()) => println!("Странно, SystemEvent должен быть не зарегистрирован"),
        Err(e) => println!("  SystemEvent не отправлен: {}", e),
    }

    println!("\nГотово! Сигналы обработаны диспетчером.");

    Ok(())
}
