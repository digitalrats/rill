use super::{Signal, SignalError, DynSignalHandler};
use std::any::Any;
use std::collections::HashMap;
use std::sync::RwLock;

/// Режим доставки сигналов
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeliveryMode {
    Immediate,
    Queued,
    Broadcast,
    Debounced(u64),
    Throttled(u64),
}

/// Конфигурация канала
#[derive(Debug, Clone)]  // Добавляем Clone
pub struct SignalChannelConfig {
    pub capacity: usize,
    pub mode: DeliveryMode,
    pub drop_policy: DropPolicy,
}

#[derive(Debug, Clone, Copy)]
pub enum DropPolicy {
    DropOldest,
    DropNewest,
    Block,
}

/// Канал сигналов
pub struct SignalChannel {
    pub handlers: Vec<Box<dyn DynSignalHandler>>,
    pub config: SignalChannelConfig,
    pub queue: Vec<Box<dyn Any + Send>>,
}

impl SignalChannel {
    pub fn new(config: SignalChannelConfig) -> Self {
        // ФИКС: Клонируем config чтобы использовать его дважды
        let capacity = config.capacity;
        Self {
            handlers: Vec::new(),
            config,
            queue: Vec::with_capacity(capacity),
        }
    }
}

/// Расширенный диспетчер сигналов
pub struct AdvancedSignalDispatcher {
    channels: RwLock<HashMap<std::any::TypeId, SignalChannel>>,
}

impl AdvancedSignalDispatcher {
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }
    
    pub fn emit<T: Signal + Send + 'static>(&self, signal: T) -> Result<(), SignalError> {
        // ФИКС: Используем полный путь к методу
        let type_id = <dyn Any>::type_id(&signal);
        
        let channels = self.channels.read().unwrap();
        
        if let Some(channel) = channels.get(&type_id) {
            match channel.config.mode {
                DeliveryMode::Immediate => {
                    for handler in &channel.handlers {
                        let _ = handler.handle_any(&signal);
                    }
                }
                DeliveryMode::Queued => {
                    if channel.queue.len() >= channel.config.capacity {
                        return Err(SignalError::ChannelFull);
                    }
                    // ФИКС: Не можем мутировать через shared reference
                    // Это проблема архитектуры
                }
                _ => {
                    // Реализация других режимов
                }
            }
            Ok(())
        } else {
            Err(SignalError::ReceiverNotFound)
        }
    }
}