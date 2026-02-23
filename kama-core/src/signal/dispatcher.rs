//! # SimpleSignalDispatcher — синхронный диспетчер сигналов

use std::any::{Any, TypeId};
use std::collections::HashMap;

use crate::signal::error::{SignalError, SignalResult};
use crate::signal::types::Signal;

/// Динамический обработчик сигналов (для type erasure)
pub trait DynSignalHandler: Send + Sync {
    /// Обработать сигнал в динамическом виде
    fn handle_any(&mut self, signal: &dyn Any) -> SignalResult<()>;
    
    /// Получить ID типа сигнала, который обрабатывает этот обработчик
    fn signal_type_id(&self) -> TypeId;
}

/// Обёртка для конкретного обработчика
pub struct SignalHandlerWrapper<T, H> {
    handler: H,
    _marker: std::marker::PhantomData<T>,
}

impl<T, H> DynSignalHandler for SignalHandlerWrapper<T, H>
where
    T: Signal + 'static,
    H: SignalHandler<T> + 'static,
{
    fn handle_any(&mut self, signal: &dyn Any) -> SignalResult<()> {
        if let Some(sig) = signal.downcast_ref::<T>() {
            self.handler.handle(sig);
            Ok(())
        } else {
            Err(SignalError::TypeMismatch)
        }
    }

    fn signal_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }
}

/// Обработчик сигналов конкретного типа
pub trait SignalHandler<T: Signal>: Send + Sync {
    /// Обработать сигнал
    fn handle(&mut self, signal: &T);
}

/// Синхронный диспетчер сигналов
pub struct SimpleSignalDispatcher {
    handlers: HashMap<TypeId, Vec<Box<dyn DynSignalHandler>>>,
}

impl SimpleSignalDispatcher {
    /// Создать новый диспетчер
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Зарегистрировать обработчик для типа T
    pub fn register<T: Signal + 'static, H: SignalHandler<T> + 'static>(
        &mut self,
        handler: H,
    ) {
        let type_id = TypeId::of::<T>();
        let wrapper = SignalHandlerWrapper {
            handler,
            _marker: std::marker::PhantomData,
        };
        self.handlers
            .entry(type_id)
            .or_insert_with(Vec::new)
            .push(Box::new(wrapper));
    }

    /// Отправить сигнал всем зарегистрированным обработчикам
    pub fn emit<T: Signal + 'static>(&mut self, signal: T) -> SignalResult<()> {
        let type_id = signal.type_id();
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

impl Default for SimpleSignalDispatcher {
    fn default() -> Self {
        Self::new()
    }
}