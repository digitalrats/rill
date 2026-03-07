//! Базовый тип очереди — неблокирующая MPMC очередь

use crossbeam_channel::{self, Receiver, Sender, TryRecvError, TrySendError};
use std::fmt;
use std::time::Duration;

/// Политика переполнения для ограниченной очереди
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverflowPolicy {
    /// Отбрасывать новые сообщения
    DropNewest,
    /// Отбрасывать самые старые сообщения
    DropOldest,
    /// Блокировать отправителя (только для control-потока)
    Block,
}

/// Базовый трейт для всех команд
///
/// Любой тип, реализующий этот трейт, может передаваться через очередь.
/// Это позволяет очередям быть полностью типобезопасными.
pub trait Command: Send + 'static + fmt::Debug {}

/// Статистика очереди
#[derive(Debug, Clone, Default)]
pub struct QueueStats {
    /// Имя очереди
    pub name: String,
    
    /// Количество отправленных сообщений
    pub sent_count: u64,
    
    /// Количество полученных сообщений
    pub received_count: u64,
    
    /// Текущий размер очереди
    pub current_size: usize,
    
    /// Максимальный размер очереди (пиковый)
    pub max_size: usize,
    
    /// Количество потерянных сообщений (если очередь переполнялась)
    pub lost_count: u64,
    
    /// Является ли очередь ограниченной
    pub is_bounded: bool,
    
    /// Емкость (для bounded очередей)
    pub capacity: Option<usize>,
}

/// Итератор по очередям
pub struct QueueIter<T: Command> {
    rx: Receiver<T>,
}

impl<T: Command> Iterator for QueueIter<T> {
    type Item = T;
    
    fn next(&mut self) -> Option<Self::Item> {
        self.rx.try_recv().ok()
    }
}

/// Неблокирующая очередь команд (Multiple Producers, Multiple Consumers)
#[derive(Clone)]
pub struct CommandQueue<T: Command> {
    tx: Sender<T>,
    rx: Receiver<T>,
    name: String,
    policy: Option<OverflowPolicy>,
    capacity: Option<usize>,
    stats: std::sync::Arc<std::sync::atomic::AtomicU64>,
    _marker: std::marker::PhantomData<T>,
}

impl<T: Command> CommandQueue<T> {
    /// Создать новую очередь с неограниченным буфером
    pub fn new(name: impl Into<String>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            tx,
            rx,
            name: name.into(),
            policy: None,
            capacity: None,
            stats: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            _marker: std::marker::PhantomData,
        }
    }
    
    /// Создать очередь с ограниченным буфером
    pub fn with_capacity(name: impl Into<String>, capacity: usize) -> Self {
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        Self {
            tx,
            rx,
            name: name.into(),
            policy: Some(OverflowPolicy::Block),
            capacity: Some(capacity),
            stats: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            _marker: std::marker::PhantomData,
        }
    }
    
    /// Создать очередь с ограниченным буфером и политикой переполнения
    pub fn with_policy(
        name: impl Into<String>, 
        capacity: usize,
        policy: OverflowPolicy,
    ) -> Self {
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        Self {
            tx,
            rx,
            name: name.into(),
            policy: Some(policy),
            capacity: Some(capacity),
            stats: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
            _marker: std::marker::PhantomData,
        }
    }
    
    /// Отправить команду (неблокирующая)
    pub fn send(&self, cmd: T) -> Result<(), TrySendError<T>> {
        match self.policy {
            Some(OverflowPolicy::DropNewest) => {
                match self.tx.try_send(cmd) {
                    Ok(()) => {
                        self.stats.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        Ok(())
                    }
                    Err(TrySendError::Full(_cmd)) => {
                        // Отбрасываем новое сообщение
                        self.stats.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            Some(OverflowPolicy::DropOldest) => {
                if self.tx.is_full() {
                    let _ = self.rx.try_recv();
                }
                match self.tx.try_send(cmd) {
                    Ok(()) => {
                        self.stats.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            _ => {
                match self.tx.try_send(cmd) {
                    Ok(()) => {
                        self.stats.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }
    
    /// Отправить команду (блокирующая)
    pub fn send_blocking(&self, cmd: T) -> Result<(), crossbeam_channel::SendError<T>> {
        let result = self.tx.send(cmd);
        if result.is_ok() {
            self.stats.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        result
    }
    
    /// Получить команду (неблокирующая)
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.rx.try_recv()
    }
    
    /// Получить команду с таймаутом
    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, crossbeam_channel::RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }
    
    /// Получить отправитель (для клонирования)
    pub fn sender(&self) -> Sender<T> {
        self.tx.clone()
    }
    
    /// Получить получатель (для клонирования)
    pub fn receiver(&self) -> Receiver<T> {
        self.rx.clone()
    }
    
    /// Имя очереди (для отладки)
    pub fn name(&self) -> &str {
        &self.name
    }
    
    /// Текущий размер очереди (приблизительный)
    pub fn len(&self) -> usize {
        self.rx.len()
    }
    
    /// Очередь пуста?
    pub fn is_empty(&self) -> bool {
        self.rx.is_empty()
    }
    
    /// Очередь переполнена? (только для bounded очередей)
    pub fn is_full(&self) -> bool {
        self.tx.is_full()
    }
    
    /// Получить итератор по всем доступным сообщениям
    pub fn iter(&self) -> QueueIter<T> {
        QueueIter { rx: self.rx.clone() }
    }
    
    /// Получить статистику
    pub fn stats(&self) -> QueueStats {
        QueueStats {
            name: self.name.clone(),
            sent_count: self.stats.load(std::sync::atomic::Ordering::Relaxed),
            received_count: 0,
            current_size: self.len(),
            max_size: 0,
            lost_count: 0,
            is_bounded: self.policy.is_some(),
            capacity: self.capacity,
        }
    }
}

impl<T: Command> fmt::Debug for CommandQueue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandQueue")
            .field("name", &self.name)
            .field("len", &self.len())
            .field("policy", &self.policy)
            .finish()
    }
}