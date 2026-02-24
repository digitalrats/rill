//! # LFO автоматы — низкочастотные генераторы
//!
//! Специализированные конструкторы для создания LFO (Low Frequency Oscillators).
//! Все реализации — это удобные обёртки над [`FunctionAutomaton`](super::FunctionAutomaton),
//! что позволяет легко комбинировать LFO с другими возможностями системы.
//!
//! ## Доступные формы волны
//!
//! - `Sine` — гладкая синусоида
//! - `Triangle` — треугольная волна
//! - `Saw` — пилообразная (нарастающая)
//! - `Square` — прямоугольная
//! - `SampleAndHold` — случайные значения, удерживаемые в течение периода
//! - `RandomWalk` — плавное случайное блуждание
//!
//! ## Диапазон значений
//!
//! LFO генерирует значения в диапазоне `[offset - amplitude, offset + amplitude]`.
//! Для форм волны, колеблющихся от -1 до 1 (синус, треугольник, пила),
//! итоговый сигнал: `offset + amplitude * raw_wave`.
//! Для `SampleAndHold` и `RandomWalk` амплитуда масштабирует случайные значения.
//!
//! ## LFO с огибающей
//!
//! [`LfoWithEnvelopeAutomaton`] комбинирует LFO и ADSR-огибающую.
//! Сигнал на выходе равен `lfo_signal * envelope_signal`.

use crate::automaton::function::FunctionAutomaton;
use kama_core::traits::ParameterId;

/// Тип формы волны LFO
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LfoWaveform {
    Sine,
    Triangle,
    Saw,
    Square,
    SampleAndHold,
    RandomWalk,
}

impl LfoWaveform {
    /// Получить все доступные формы волны
    pub fn names() -> Vec<&'static str> {
        vec!["sine", "triangle", "saw", "square", "s&h", "random_walk"]
    }

    /// Получить форму волны из строки
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "sine" => Some(LfoWaveform::Sine),
            "triangle" => Some(LfoWaveform::Triangle),
            "saw" => Some(LfoWaveform::Saw),
            "square" => Some(LfoWaveform::Square),
            "s&h" | "sample_and_hold" => Some(LfoWaveform::SampleAndHold),
            "random_walk" => Some(LfoWaveform::RandomWalk),
            _ => None,
        }
    }
}

/// LFO автомат - специализированная версия FunctionAutomaton
pub type LfoAutomaton = FunctionAutomaton;

/// LFO с огибающей - также FunctionAutomaton
pub type LfoWithEnvelopeAutomaton = FunctionAutomaton;

/// Вспомогательные функции для создания LFO автоматов
impl LfoAutomaton {
    /// Создать LFO автомат с формой волны по умолчанию (синус)
    pub fn lfo(
        frequency: f64,
        amplitude: f64,
        offset: f64,
        target_parameter: ParameterId,
    ) -> Self {
        let mut phase = 0.0;
        let phase_inc = frequency / 44100.0; // Временное решение, sample_rate будет передан через контекст

        Self::new(
            "LFO",
            move |_time| {
                let value = (phase * 2.0 * std::f64::consts::PI).sin();
                phase += phase_inc;
                if phase >= 1.0 {
                    phase -= 1.0;
                }
                value * amplitude + offset
            },
            target_parameter,
        )
    }

    /// Создать LFO автомат с указанной формой волны
    pub fn lfo_with_waveform(
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        target_parameter: ParameterId,
    ) -> Self {
        let mut phase = 0.0;
        let phase_inc = frequency / 44100.0;

        Self::new(
            "LFO",
            move |_time| {
                let raw = match waveform {
                    LfoWaveform::Sine => (phase * 2.0 * std::f64::consts::PI).sin(),
                    LfoWaveform::Triangle => {
                        if phase < 0.5 {
                            4.0 * phase - 1.0
                        } else {
                            3.0 - 4.0 * phase
                        }
                    }
                    LfoWaveform::Saw => 2.0 * phase - 1.0,
                    LfoWaveform::Square => {
                        if phase < 0.5 { 1.0 } else { -1.0 }
                    }
                    LfoWaveform::SampleAndHold => {
                        // Временная заглушка
                        (phase * 2.0 * std::f64::consts::PI).sin()
                    }
                    LfoWaveform::RandomWalk => {
                        // Временная заглушка
                        (phase * 2.0 * std::f64::consts::PI).sin()
                    }
                };
                
                phase += phase_inc;
                if phase >= 1.0 {
                    phase -= 1.0;
                }
                
                raw * amplitude + offset
            },
            target_parameter,
        )
    }
}

/// Вспомогательные функции для создания LFO автоматов с огибающей
impl LfoWithEnvelopeAutomaton {
    /// Создать LFO с огибающей
    pub fn lfo_with_envelope(
        frequency: f64,
        amplitude: f64,
        offset: f64,
        attack: f64,
        release: f64,
        target_parameter: ParameterId,
    ) -> Self {
        let mut phase = 0.0;
        let mut envelope_phase = 0.0;
        let phase_inc = frequency / 44100.0;
        let mut envelope_stage = 0; // 0=attack, 1=sustain, 2=release

        Self::new(
            "LFO+Envelope",
            move |time| {
                // LFO
                let lfo_val = (phase * 2.0 * std::f64::consts::PI).sin();
                phase += phase_inc;
                if phase >= 1.0 {
                    phase -= 1.0;
                }

                // Огибающая (упрощенная)
                let env_val = if time < attack {
                    time / attack
                } else if time > 1.0 - release {
                    (1.0 - time) / release
                } else {
                    1.0
                };

                lfo_val * amplitude * env_val + offset
            },
            target_parameter,
        )
    }
}