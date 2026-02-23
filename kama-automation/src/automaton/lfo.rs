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

//! LFO автомат (обёртка над FunctionAutomaton)

use crate::automaton::function::FunctionAutomaton;
use kama_oscillators::control::{Lfo, LfoWaveform};
use std::sync::{Arc, Mutex};

/// LFO автомат - специализированная версия FunctionAutomaton
pub type LfoAutomaton = FunctionAutomaton;

/// LFO с огибающей - также FunctionAutomaton
pub type LfoWithEnvelopeAutomaton = FunctionAutomaton;

/// Вспомогательные функции для создания LFO автоматов
impl LfoAutomaton {
    /// Создать LFO автомат из параметров
    /// Создать LFO автомат с формой волны по умолчанию (синус).
    ///
    /// # Аргументы
    /// * `frequency` — частота в Hz (0.01–100)
    /// * `amplitude` — амплитуда (0.0–1.0)
    /// * `offset` — смещение (-1.0–1.0)
    pub fn lfo(
        frequency: f64,
        amplitude: f64,
        offset: f64,
        target_node: &str,
        target_param: &str,
    ) -> Self {
        let lfo = Arc::new(Mutex::new(Lfo::new(frequency, amplitude, offset)));

        Self::new(
            "LFO",
            move |_time| {
                let mut lfo = lfo.lock().unwrap();
                lfo.generate()
            },
            target_node,
            target_param,
        )
    }

    /// Создать LFO автомат с указанной формой волны
    /// Создать LFO автомат с указанной формой волны.
    pub fn lfo_with_waveform(
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        target_node: &str,
        target_param: &str,
    ) -> Self {
        let lfo = Arc::new(Mutex::new(
            Lfo::new(frequency, amplitude, offset).with_waveform(waveform),
        ));

        Self::new(
            "LFO",
            move |_time| {
                let mut lfo = lfo.lock().unwrap();
                lfo.generate()
            },
            target_node,
            target_param,
        )
    }

    /// Создать LFO автомат с возможностью сброса
    /// Создать LFO автомат с возможностью сброса фазы при t=0.
    pub fn lfo_with_reset(
        frequency: f64,
        amplitude: f64,
        offset: f64,
        target_node: &str,
        target_param: &str,
    ) -> Self {
        let lfo = Arc::new(Mutex::new(Lfo::new(frequency, amplitude, offset)));

        Self::new(
            "LFO",
            move |time| {
                let mut lfo = lfo.lock().unwrap();
                if time == 0.0 {
                    lfo.reset();
                }
                lfo.generate()
            },
            target_node,
            target_param,
        )
    }
}

/// Вспомогательные функции для создания LFO автоматов с огибающей
impl LfoWithEnvelopeAutomaton {
    /// Создать LFO с огибающей
    /// Создать LFO с огибающей.
    pub fn lfo_with_envelope(
        frequency: f64,
        amplitude: f64,
        offset: f64,
        attack: f64,
        release: f64,
        target_node: &str,
        target_param: &str,
    ) -> Self {
        use kama_oscillators::control::Envelope;

        let lfo = Arc::new(Mutex::new(Lfo::new(frequency, amplitude, offset)));
        let envelope = Arc::new(Mutex::new(Envelope::new(attack, 0.0, 1.0, release)));

        Self::new(
            "LFO+Envelope",
            move |time| {
                let mut lfo = lfo.lock().unwrap();
                let mut envelope = envelope.lock().unwrap();

                if time == 0.0 {
                    envelope.trigger();
                }

                let lfo_val = lfo.generate();
                let env_val = envelope.generate();
                lfo_val * env_val
            },
            target_node,
            target_param,
        )
    }

    /// Создать LFO с огибающей и конкретной формой волны
    pub fn lfo_with_envelope_and_waveform(
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
        attack: f64,
        release: f64,
        target_node: &str,
        target_param: &str,
    ) -> Self {
        use kama_oscillators::control::Envelope;

        let lfo = Arc::new(Mutex::new(
            Lfo::new(frequency, amplitude, offset).with_waveform(waveform),
        ));
        let envelope = Arc::new(Mutex::new(Envelope::new(attack, 0.0, 1.0, release)));

        Self::new(
            "LFO+Envelope",
            move |time| {
                let mut lfo = lfo.lock().unwrap();
                let mut envelope = envelope.lock().unwrap();

                if time == 0.0 {
                    envelope.trigger();
                }

                let lfo_val = lfo.generate();
                let env_val = envelope.generate();
                lfo_val * env_val
            },
            target_node,
            target_param,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automaton::Automaton;
    use crate::context::AutomationContext;
    use float_cmp::approx_eq;

    #[test]
    fn test_lfo_automaton_creation() {
        let automaton = LfoAutomaton::lfo(1.0, 0.5, 0.0, "test", "param");
        assert_eq!(automaton.name(), "LFO");
    }

    #[test]
    fn test_lfo_automaton_step() {
        let automaton = LfoAutomaton::lfo(1.0, 0.5, 0.0, "test", "param");

        let state = automaton.initial_state();
        println!("Initial state value: {:.6}", state.value);

        // Собираем несколько значений
        let mut values = Vec::new();
        let mut current_state = state;

        for i in 1..=10 {
            let time = i as f64 * 0.1;
            let (new_state, _) =
                automaton.step(time, &AutomationContext::dummy(), (), &current_state);
            println!("Step {} (t={:.1}): value = {:.6}", i, time, new_state.value);
            values.push(new_state.value);
            current_state = new_state;
        }

        // Проверяем, что значения действительно меняются (разница между первым и последним > 0)
        let first = values[0];
        let last = values[values.len() - 1];
        println!(
            "First value: {:.6}, Last value: {:.6}, Difference: {:.6}",
            first,
            last,
            (last - first).abs()
        );

        // Значения должны отличаться друг от друга (пусть даже очень мало)
        assert!(
            (last - first).abs() > 0.0,
            "LFO values should change over time"
        );

        // Можно также проверить монотонность (для sine LFO в начале фазы)
        for i in 1..values.len() {
            assert!(
                values[i] > values[i - 1],
                "Values should be increasing: {:.6} -> {:.6}",
                values[i - 1],
                values[i]
            );
        }
    }

    #[test]
    fn test_lfo_with_waveform_creation() {
        let automaton =
            LfoAutomaton::lfo_with_waveform(1.0, 0.5, 0.0, LfoWaveform::Square, "test", "param");
        assert_eq!(automaton.name(), "LFO");
    }

    #[test]
    fn test_lfo_with_envelope_creation() {
        let automaton =
            LfoWithEnvelopeAutomaton::lfo_with_envelope(1.0, 0.5, 0.0, 0.1, 0.2, "test", "param");
        assert_eq!(automaton.name(), "LFO+Envelope");
    }
}
