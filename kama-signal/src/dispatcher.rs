use std::any::{Any, TypeId};
use std::collections::HashMap;
use parking_lot::RwLock;
use crate::error::{SignalError, SignalResult};
use crate::Signal;

/// Динамический обработчик сигналов
pub trait DynSignalHandler: Send + Sync {
    fn handle_any(&mut self, signal: &dyn Any) -> SignalResult<()>;
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
    fn handle(&mut self, signal: &T);
}

/// Простой диспетчер сигналов (синхронный)
pub struct SimpleSignalDispatcher {
    handlers: HashMap<TypeId, Vec<Box<dyn DynSignalHandler>>>,
}

impl SimpleSignalDispatcher {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

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