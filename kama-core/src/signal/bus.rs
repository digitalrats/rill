//! # SignalBus — многопоточная шина сигналов
//!
//! Предоставляет MPMC (Multiple Producers, Multiple Consumers) канал
//! для передачи сигналов между компонентами системы.

use crossbeam_channel::{self, Receiver, Sender, TrySendError};
use crate::signal::error::{SignalError, SignalResult};
use crate::signal::types::Signal;

/// Политика поведения при переполнении ограниченной очереди.
#[derive(Debug, Clone, Copy)]
pub enum OverflowPolicy {
    /// Отбрасывать новые сообщения
    DropNewest,
    /// Отбрасывать самые старые сообщения
    DropOldest,
    /// Блокировать отправителя до появления места
    Block,
}

/// Конфигурация шины сигналов.
#[derive(Debug, Clone)]
pub enum BusConfig {
    /// Ограниченная очередь с политикой
    Bounded(usize, OverflowPolicy),
    /// Неограниченная очередь
    Unbounded,
}

/// Многопоточная шина сигналов определённого типа.
///
/// Поддерживает множество отправителей и множество получателей.
/// Каждое сообщение доставляется ровно одному получателю.
#[derive(Debug, Clone)]
pub struct SignalBus<T: Signal> {
    tx: Sender<T>,
    rx: Receiver<T>,
    config: BusConfig,
}

impl<T: Signal> SignalBus<T> {
    /// Создать новую шину с заданной конфигурацией.
    pub fn new(config: BusConfig) -> Self {
        let (tx, rx) = match config {
            BusConfig::Bounded(cap, _) => crossbeam_channel::bounded(cap),
            BusConfig::Unbounded => crossbeam_channel::unbounded(),
        };
        Self { tx, rx, config }
    }

    /// Отправить сигнал в шину.
    pub fn send(&self, signal: T) -> SignalResult<()> {
        match self.config {
            BusConfig::Bounded(_, policy) => match policy {
                OverflowPolicy::DropNewest => {
                    self.tx.try_send(signal).map_err(|e| match e {
                        TrySendError::Full(_) => SignalError::ChannelFull,
                        TrySendError::Disconnected(_) => SignalError::Disconnected,
                    })?;
                    Ok(())
                }
                OverflowPolicy::DropOldest => {
                    if self.tx.is_full() {
                        let _ = self.rx.try_recv();
                    }
                    self.tx.send(signal).map_err(|_| SignalError::Disconnected)?;
                    Ok(())
                }
                OverflowPolicy::Block => {
                    self.tx.send(signal).map_err(|_| SignalError::Disconnected)?;
                    Ok(())
                }
            },
            BusConfig::Unbounded => {
                self.tx.send(signal).map_err(|_| SignalError::Disconnected)?;
                Ok(())
            }
        }
    }

    /// Попытаться получить сигнал из шины без блокировки.
    pub fn try_recv(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    /// Получить клонированного получателя.
    pub fn receiver(&self) -> Receiver<T> {
        self.rx.clone()
    }

    /// Получить клонированного отправителя.
    pub fn sender(&self) -> Sender<T> {
        self.tx.clone()
    }
}