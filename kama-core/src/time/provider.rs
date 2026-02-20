// kama-core/src/time/provider.rs
//! Расширенный источник времени с поддержкой темпа и метронома.

use std::fmt::Debug;
use super::{Clock, TickInfo};

/// Провайдер времени, дополняющий `Clock` информацией о темпе и тактах.
pub trait TimeProvider: Clock + Debug {
    /// Текущий темп (BPM).
    fn bpm(&self) -> f64;

    /// Установить темп.
    fn set_bpm(&self, bpm: f64);

    /// Получить детальную информацию о текущем такте/доле.
    ///
    /// Значения вычисляются на основе текущей позиции, BPM и частоты дискретизации.
    fn tick_info(&self) -> TickInfo;
}