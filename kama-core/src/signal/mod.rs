//! Сигнальная система для коммуникации между компонентами

use std::any::{Any, TypeId};
use serde::{Serialize, Deserialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignalError {
    #[error("Signal type mismatch")]
    TypeMismatch,
    #[error("Receiver not found")]
    ReceiverNotFound,
    #[error("Signal channel full")]
    ChannelFull,
}

/// Базовый трейт сигнала
pub trait Signal: Any + Send + Sync {
    fn type_id(&self) -> TypeId {
        TypeId::of::<Self>()
    }
}

/// Типы сигналов
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterChanged {
    pub node_id: String,
    pub parameter_id: String,
    pub value: f32,
    pub normalized_value: f32,
    pub timestamp: u64,
    pub source: SignalSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalSource {
    UserInterface,
    Automation,
    Midi { channel: u8, controller: u8 },
    Osc { address: String },
    Script,
    External,
}

impl Signal for ParameterChanged {}
impl Signal for SignalSource {}

/// Обработчик сигналов
pub trait SignalHandler<T: Signal>: Send + Sync {
    fn handle(&mut self, signal: &T);
}

/// Динамический обработчик
pub trait DynSignalHandler: Send + Sync {
    fn handle_any(&mut self, signal: &dyn Any) -> Result<(), SignalError>;
    fn signal_type_id(&self) -> TypeId;
}

// Создаем обертку
pub struct SignalHandlerWrapper<T, H> {
    handler: H,
    _marker: std::marker::PhantomData<T>,
}

impl<T, H> DynSignalHandler for SignalHandlerWrapper<T, H>
where
    T: Signal + 'static,
    H: SignalHandler<T> + 'static,
{
    fn handle_any(&mut self, signal: &dyn Any) -> Result<(), SignalError> {
        if let Some(sig) = signal.downcast_ref::<T>() {
            self.handler.handle(sig); // ФИКС: добавили .handler
            Ok(())
        } else {
            Err(SignalError::TypeMismatch)
        }
    }
    
    fn signal_type_id(&self) -> TypeId { // ФИКС: переименовали метод в трейте и тут
        TypeId::of::<T>()
    }
}

/// Простой диспетчер сигналов
pub struct SimpleSignalDispatcher {
    handlers: std::collections::HashMap<TypeId, Vec<Box<dyn DynSignalHandler>>>,
}

impl SimpleSignalDispatcher {
    pub fn new() -> Self {
        Self {
            handlers: std::collections::HashMap::new(),
        }
    }
    
    pub fn register<T: Signal + 'static, H: SignalHandler<T> + 'static>(
        &mut self,
        handler: H,
    ) {
        let type_id = TypeId::of::<T>();
        // ФИКС: Создаем обертку перед помещением в Box
        let wrapper = SignalHandlerWrapper {
            handler,
            _marker: std::marker::PhantomData,
        };
        
        self.handlers
            .entry(type_id)
            .or_insert_with(Vec::new)
            .push(Box::new(wrapper)); 
    }
    
    pub fn emit<T: Signal + 'static>(&mut self, signal: T) -> Result<(), SignalError> {
        // ФИКС: Явный вызов метода вашего трейта
        let type_id = Signal::type_id(&signal);
        
        if let Some(handlers) = self.handlers.get_mut(&type_id) {
            for handler in handlers {
                handler.handle_any(&signal)?;
            }
            Ok(())
        } else {
            Err(SignalError::ReceiverNotFound)
        }
    }

}