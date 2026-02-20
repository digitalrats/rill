use crossbeam_channel::{self, Receiver, Sender, TrySendError};
use crate::error::{SignalError, SignalResult};

/// Политика переполнения канала
#[derive(Debug, Clone, Copy)]
pub enum OverflowPolicy {
    DropNewest,
    DropOldest,
    Block,
}

/// Конфигурация шины сигналов
#[derive(Debug, Clone)]
pub enum BusConfig {
    Bounded(usize, OverflowPolicy),
    Unbounded,
}

/// Шина сигналов для определённого типа
#[derive(Debug, Clone)]
pub struct SignalBus<T: crate::Signal> {
    tx: Sender<T>,
    rx: Receiver<T>,
    config: BusConfig,
}

impl<T: crate::Signal> SignalBus<T> {
    pub fn new(config: BusConfig) -> Self {
        let (tx, rx) = match config {
            BusConfig::Bounded(cap, _) => crossbeam_channel::bounded(cap),
            BusConfig::Unbounded => crossbeam_channel::unbounded(),
        };
        Self { tx, rx, config }
    }

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

    pub fn try_recv(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }

    pub fn receiver(&self) -> Receiver<T> {
        self.rx.clone()
    }

    pub fn sender(&self) -> Sender<T> {
        self.tx.clone()
    }
}