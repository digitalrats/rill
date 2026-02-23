//! # SimpleSignalDispatcher — синхронный диспетчер сигналов
//!
//! Предоставляет механизм регистрации обработчиков для разных типов сигналов
//! и синхронной диспетчеризации сигналов всем зарегистрированным обработчикам.
//!
//! ## Особенности
//!
//! - Типобезопасность через generics
//! - Множество обработчиков на один тип сигнала
//! - Синхронная обработка (все обработчики вызываются в том же потоке)

use crate::error::{SignalError, SignalResult};
use crate::Signal;
use parking_lot::RwLock;
use std::any::{Any, TypeId};
use std::collections::HashMap;

/// Динамический обработчик сигналов
/// Динамический обработчик сигналов (для type erasure).
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
/// Обработчик сигналов конкретного типа.
///
/// Реализуется для типов, которые хотят получать сигналы определённого типа.
pub trait SignalHandler<T: Signal>: Send + Sync {
    fn handle(&mut self, signal: &T);
}

/// Простой диспетчер сигналов (синхронный)
/// Синхронный диспетчер сигналов.
///
/// Хранит обработчики для разных типов сигналов и вызывает их
/// при получении сигнала соответствующего типа.
pub struct SimpleSignalDispatcher {
    handlers: HashMap<TypeId, Vec<Box<dyn DynSignalHandler>>>,
}

impl SimpleSignalDispatcher {
    /// Создать новый диспетчер.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn register<T: Signal + 'static, H: SignalHandler<T> + 'static>(&mut self, handler: H) {
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
