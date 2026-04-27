//! # Утилиты для патчбэя
//!
//! Вспомогательные функции и структуры для работы с патчбэем:
//! - Конвертеры значений
//! - Утилиты для времени
//! - Хелперы для тестирования

use crate::automaton::Range;
use crate::control::Transform;

// =============================================================================
// Конвертеры значений
// =============================================================================

/// Конвертер значений между разными шкалами
#[derive(Debug, Clone)]
pub struct ValueConverter {
    /// Входной диапазон
    input_range: Range,
    /// Выходной диапазон
    output_range: Range,
    /// Тип преобразования
    transform: Transform,
}

impl ValueConverter {
    /// Создать новый конвертер
    pub fn new(input_range: Range, output_range: Range, transform: Transform) -> Self {
        Self {
            input_range,
            output_range,
            transform,
        }
    }

    /// Конвертировать значение
    pub fn convert(&self, value: f64) -> f64 {
        // Нормализуем входное значение
        let norm = self.input_range.normalize(value);

        // Применяем преобразование
        let transformed = match self.transform {
            Transform::Linear => norm,
            Transform::Exponential => norm * norm,
            Transform::Logarithmic => (1.0 + norm * 9.0).log10(),
            Transform::Inverted => 1.0 - norm,
            Transform::Custom(ref f) => f(norm as f32) as f64,
        };

        // Денормализуем в выходной диапазон
        self.output_range.denormalize(transformed)
    }

    /// Конвертировать значение в обратном направлении
    pub fn convert_inverse(&self, value: f64) -> f64 {
        // Денормализуем в обратную сторону (приблизительно)
        let norm = self.output_range.normalize(value);
        self.input_range.denormalize(norm)
    }
}

/// Преобразование MIDI value (0-127) в нормализованное значение (0.0-1.0)
pub fn midi_to_normalized(midi: u8) -> f64 {
    midi as f64 / 127.0
}

/// Преобразование нормализованного значения в MIDI value
pub fn normalized_to_midi(norm: f64) -> u8 {
    (norm.clamp(0.0, 1.0) * 127.0).round() as u8
}

/// Преобразование частоты в MIDI ноту
pub fn freq_to_midi_note(freq: f64) -> f64 {
    69.0 + 12.0 * (freq / 440.0).log2()
}

/// Преобразование MIDI ноты в частоту
pub fn midi_note_to_freq(note: f64) -> f64 {
    440.0 * 2.0_f64.powf((note - 69.0) / 12.0)
}

// =============================================================================
// Утилиты для времени
// =============================================================================

/// Метроном для синхронизации с BPM
#[derive(Debug, Clone)]
pub struct Metronome {
    /// BPM
    bpm: f64,
    /// Время последнего тика
    last_tick: f64,
    /// Время следующего тика
    next_tick: f64,
    /// Длительность четверти в секундах
    quarter_duration: f64,
}

impl Metronome {
    /// Создать новый метроном
    pub fn new(bpm: f64) -> Self {
        let quarter_duration = 60.0 / bpm;
        Self {
            bpm,
            last_tick: 0.0,
            next_tick: quarter_duration,
            quarter_duration,
        }
    }

    /// Обновить состояние и проверить, был ли тик
    pub fn update(&mut self, time: f64) -> bool {
        if time >= self.next_tick {
            self.last_tick = self.next_tick;
            self.next_tick += self.quarter_duration;
            true
        } else {
            false
        }
    }

    /// Получить текущую фазу (0.0-1.0) внутри четверти
    pub fn phase(&self, time: f64) -> f64 {
        ((time - self.last_tick) / self.quarter_duration).clamp(0.0, 1.0)
    }

    /// Установить новый BPM
    pub fn set_bpm(&mut self, bpm: f64) {
        self.bpm = bpm;
        self.quarter_duration = 60.0 / bpm;
        self.next_tick = self.last_tick + self.quarter_duration;
    }

    /// Сбросить метроном
    pub fn reset(&mut self) {
        self.last_tick = 0.0;
        self.next_tick = self.quarter_duration;
    }
}

/// Преобразование длительности ноты в секунды
pub fn note_duration_to_seconds(note_type: NoteType, bpm: f64) -> f64 {
    let quarter = 60.0 / bpm;
    match note_type {
        NoteType::Whole => quarter * 4.0,
        NoteType::Half => quarter * 2.0,
        NoteType::Quarter => quarter,
        NoteType::Eighth => quarter / 2.0,
        NoteType::Sixteenth => quarter / 4.0,
        NoteType::ThirtySecond => quarter / 8.0,
        NoteType::Dotted(n) => note_duration_to_seconds(*n, bpm) * 1.5,
        NoteType::Triplet(n) => note_duration_to_seconds(*n, bpm) * 2.0 / 3.0,
    }
}

/// Тип ноты
#[derive(Debug, Clone)]
pub enum NoteType {
    Whole,
    Half,
    Quarter,
    Eighth,
    Sixteenth,
    ThirtySecond,
    Dotted(Box<NoteType>),
    Triplet(Box<NoteType>),
}

// =============================================================================
// Хелперы для тестирования
// =============================================================================

/// Запись событий для тестирования
#[derive(Debug, Default)]
pub struct EventRecorder {
    /// Записанные события
    events: Vec<RecordedEvent>,
}

/// Записанное событие
#[derive(Debug, Clone)]
pub struct RecordedEvent {
    /// Время записи
    pub time: f64,
    /// Тип события
    pub event_type: String,
    /// Значение
    pub value: f64,
    /// Дополнительные данные
    pub data: String,
}

impl EventRecorder {
    /// Создать новый рекордер
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Записать событие
    pub fn record(&mut self, time: f64, event_type: &str, value: f64, data: &str) {
        self.events.push(RecordedEvent {
            time,
            event_type: event_type.to_string(),
            value,
            data: data.to_string(),
        });
    }

    /// Получить все события
    pub fn events(&self) -> &[RecordedEvent] {
        &self.events
    }

    /// Очистить запись
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Найти события по типу
    pub fn find_by_type(&self, event_type: &str) -> Vec<&RecordedEvent> {
        self.events
            .iter()
            .filter(|e| e.event_type == event_type)
            .collect()
    }
}

// =============================================================================
// Генераторы тестовых сигналов
// =============================================================================

/// Генератор тестовых сигналов
pub struct TestSignalGenerator {
    /// Тип сигнала
    signal_type: TestSignalType,
    /// Параметры
    params: TestSignalParams,
}

/// Тип тестового сигнала
#[derive(Debug, Clone)]
pub enum TestSignalType {
    /// Синусоида
    Sine,
    /// Прямоугольный
    Square,
    /// Пилообразный
    Saw,
    /// Случайный шум
    Noise,
    /// Огибающая ADSR
    Envelope,
}

/// Параметры тестового сигнала
#[derive(Debug, Clone)]
pub struct TestSignalParams {
    /// Частота (Гц)
    pub frequency: f64,
    /// Амплитуда
    pub amplitude: f64,
    /// Смещение
    pub offset: f64,
    /// Длительность (секунды)
    pub duration: f64,
}

impl TestSignalGenerator {
    /// Создать новый генератор
    pub fn new(signal_type: TestSignalType, params: TestSignalParams) -> Self {
        Self {
            signal_type,
            params,
        }
    }

    /// Генерировать значение в заданное время
    pub fn generate(&self, time: f64) -> f64 {
        if time > self.params.duration {
            return 0.0;
        }

        match self.signal_type {
            TestSignalType::Sine => {
                let phase = 2.0 * std::f64::consts::PI * self.params.frequency * time;
                self.params.offset + self.params.amplitude * phase.sin()
            }

            TestSignalType::Square => {
                let phase = (self.params.frequency * time) % 1.0;
                let value = if phase < 0.5 { 1.0 } else { -1.0 };
                self.params.offset + self.params.amplitude * value
            }

            TestSignalType::Saw => {
                let phase = (self.params.frequency * time) % 1.0;
                let value = 2.0 * phase - 1.0;
                self.params.offset + self.params.amplitude * value
            }

            TestSignalType::Noise => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                self.params.offset + self.params.amplitude * (rng.gen::<f64>() * 2.0 - 1.0)
            }

            TestSignalType::Envelope => {
                // Простая ADSR-подобная огибающая
                let attack = 0.1;
                let decay = 0.2;
                let sustain = 0.7;
                let release = 0.3;

                if time < attack {
                    (time / attack) * self.params.amplitude
                } else if time < attack + decay {
                    (1.0 - (1.0 - sustain) * ((time - attack) / decay)) * self.params.amplitude
                } else if time < self.params.duration - release {
                    sustain * self.params.amplitude
                } else {
                    let rel_time = time - (self.params.duration - release);
                    (sustain * (1.0 - rel_time / release)) * self.params.amplitude
                }
            }
        }
    }
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_converter() {
        let converter = ValueConverter::new(
            Range::new(0.0, 127.0),
            Range::new(0.0, 1.0),
            Transform::Linear,
        );

        let result = converter.convert(64.0);
        assert!((result - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_metronome() {
        let mut metro = Metronome::new(120.0); // 120 BPM = 0.5 сек на четверть

        assert!(!metro.update(0.2));
        assert!(metro.update(0.6));
        assert!((metro.phase(0.6) - 0.2).abs() < 0.01);
    }

    #[test]
    fn test_test_signal() {
        let params = TestSignalParams {
            frequency: 1.0,
            amplitude: 1.0,
            offset: 0.0,
            duration: 2.0,
        };

        let gen = TestSignalGenerator::new(TestSignalType::Sine, params);
        let val = gen.generate(0.25);
        assert!((val - 1.0).abs() < 0.01);
    }
}
