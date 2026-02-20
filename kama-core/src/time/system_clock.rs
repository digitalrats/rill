//! Реализация `TimeProvider` на основе атомарных счётчиков.

use std::fmt::Debug; // <-- Добавляем импорт Debug
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Clock, TimeProvider, TickInfo};

/// Системные часы – эталонная реализация `TimeProvider`.
///
/// Потокобезопасны (lock-free), могут использоваться в аудиопотоке.
pub struct SystemClock {
    sample_rate: f64,
    position: AtomicU64,
    bpm: AtomicU64, // храним биты f64 для атомарности
}

impl SystemClock {
    /// Создать новые часы с заданной частотой дискретизации и начальным BPM.
    pub fn new(sample_rate: f64, initial_bpm: f64) -> Self {
        Self {
            sample_rate,
            position: AtomicU64::new(0),
            bpm: AtomicU64::new(initial_bpm.to_bits()),
        }
    }
}

impl Clock for SystemClock {
    fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    fn position_samples(&self) -> u64 {
        self.position.load(Ordering::Relaxed)
    }


    fn advance(&self, samples: u64) -> u64 {
        self.position.fetch_add(samples, Ordering::Relaxed)
    }

    fn reset(&self) {
        self.position.store(0, Ordering::Relaxed);
    }
}

impl TimeProvider for SystemClock {
    fn bpm(&self) -> f64 {
        f64::from_bits(self.bpm.load(Ordering::Relaxed))
    }

    fn set_bpm(&self, bpm: f64) {
        self.bpm.store(bpm.to_bits(), Ordering::Relaxed);
    }

    fn tick_info(&self) -> TickInfo {
        let pos = self.position_samples();
        let sr = self.sample_rate();
        let bpm = self.bpm();

        // Количество сэмплов на одну долю (четверть)
        let samples_per_beat = (60.0 / bpm * sr) as u64;
        if samples_per_beat == 0 {
            return TickInfo {
                bar: 0,
                beat: 0,
                sixteenth: 0,
                sample_pos: pos,
            };
        }

        // Общее количество долей (с учётом плавающей точки для точности)
        let total_beats_f = pos as f64 / samples_per_beat as f64;
        let total_beats = total_beats_f.floor() as u64;
        
        // Доля внутри такта (0-3)
        let beat_in_bar = (total_beats % 4) as u8;
        
        // Номер такта
        let bar = (total_beats / 4) as u32;

        // Сэмплы внутри текущей доли
        let samples_in_beat = pos - (total_beats * samples_per_beat);
        let sixteenth = (samples_in_beat * 4 / samples_per_beat) as u8;

        TickInfo {
            bar,
            beat: beat_in_bar,
            sixteenth,
            sample_pos: pos,
        }
    }
}

// Реализация Debug для SystemClock
impl Debug for SystemClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemClock")
            .field("sample_rate", &self.sample_rate)
            .field("position", &self.position.load(Ordering::Relaxed))
            .field("bpm", &self.bpm())
            .finish()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_clock_clock_trait() {
        let clock = SystemClock::new(44100.0, 120.0);
        assert_eq!(clock.sample_rate(), 44100.0);
        assert_eq!(clock.position_samples(), 0);
        assert_eq!(clock.position_seconds(), 0.0);
    }

    #[test]
    fn test_system_clock_time_provider() {
        let clock = SystemClock::new(44100.0, 120.0);
        clock.advance(44100);
        assert_eq!(clock.position_samples(), 44100);
        assert!((clock.position_seconds() - 1.0).abs() < 1e-6);

        clock.set_bpm(140.0);
        assert_eq!(clock.bpm(), 140.0);
    }

        #[test]
    fn test_tick_info() {
        let clock = SystemClock::new(44100.0, 120.0);
        
        // При 120 BPM одна доля = 0.5 сек = 22050 сэмплов
        clock.advance(22050); // прошла одна доля
        let info = clock.tick_info();
        assert_eq!(info.beat, 1); // вторая доля (0-индексация)
        assert_eq!(info.bar, 0);
        assert_eq!(info.sixteenth, 0);

        clock.advance(22050 * 2); // ещё две доли – всего три доли
        let info = clock.tick_info();
        assert_eq!(info.beat, 3); // четвёртая доля
        assert_eq!(info.bar, 0);

        clock.advance(22050); // ещё одна доля – прошёл полный такт
        let info = clock.tick_info();
        assert_eq!(info.beat, 0); // начало нового такта
        assert_eq!(info.bar, 1);
    }
}