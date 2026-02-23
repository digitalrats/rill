//! Временные заглушки для контекста
//! В реальном использовании будут заменены на настоящие реализации

use kama_core::traits::time::{Clock, TickInfo, TimeProvider};

/// Заглушка для TimeProvider
#[derive(Debug)]
pub(crate) struct DummyTimeProvider;

impl Clock for DummyTimeProvider {
    fn sample_rate(&self) -> f64 {
        44100.0
    }
    fn position_samples(&self) -> u64 {
        0
    }
    fn advance(&self, _samples: u64) -> u64 {
        0
    }
    fn reset(&self) {}
}

impl TimeProvider for DummyTimeProvider {
    fn bpm(&self) -> f64 {
        120.0
    }
    fn set_bpm(&self, _bpm: f64) {}
    fn tick_info(&self) -> TickInfo {
        TickInfo {
            bar: 0,
            beat: 0,
            sixteenth: 0,
            sample_pos: 0,
        }
    }
}
