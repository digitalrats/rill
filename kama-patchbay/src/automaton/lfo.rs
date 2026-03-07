//! # LFO (Low Frequency Oscillator) автоматы
//!
//! Генераторы периодических сигналов для модуляции параметров.
//! Поддерживаются различные формы волны и режимы синхронизации.

use super::{Automaton, Time, Range, SyncMode, NoAction};
use std::f64::consts::PI;

/// Форма волны LFO
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LfoWaveform {
    /// Синусоида (гладкая)
    Sine,
    /// Треугольная волна
    Triangle,
    /// Пилообразная волна (нарастающая)
    Saw,
    /// Пилообразная волна (спадающая)
    ReverseSaw,
    /// Прямоугольная волна
    Square,
    /// Прямоугольная с переменной скважностью
    Pulse(f64),  // 0.0 - 1.0
    /// Случайное значение, удерживаемое в течение периода
    SampleAndHold,
    /// Плавное случайное блуждание
    RandomWalk,
}

impl LfoWaveform {
    /// Получить название формы волны
    pub fn name(&self) -> &'static str {
        match self {
            LfoWaveform::Sine => "Sine",
            LfoWaveform::Triangle => "Triangle",
            LfoWaveform::Saw => "Saw",
            LfoWaveform::ReverseSaw => "Reverse Saw",
            LfoWaveform::Square => "Square",
            LfoWaveform::Pulse(_) => "Pulse",
            LfoWaveform::SampleAndHold => "S&H",
            LfoWaveform::RandomWalk => "Random Walk",
        }
    }
    
    /// Вычислить значение для заданной фазы
    pub fn evaluate(&self, phase: f64, pulse_width: Option<f64>) -> f64 {
        match self {
            LfoWaveform::Sine => (phase * 2.0 * PI).sin(),
            
            LfoWaveform::Triangle => {
                if phase < 0.25 {
                    4.0 * phase
                } else if phase < 0.75 {
                    2.0 - 4.0 * phase
                } else {
                    4.0 * phase - 4.0
                }
            }
            
            LfoWaveform::Saw => 2.0 * phase - 1.0,
            
            LfoWaveform::ReverseSaw => 1.0 - 2.0 * phase,
            
            LfoWaveform::Square => {
                if phase < 0.5 { 1.0 } else { -1.0 }
            }
            
            LfoWaveform::Pulse(width) => {
                let w = pulse_width.unwrap_or(*width);
                if phase < w { 1.0 } else { -1.0 }
            }
            
            LfoWaveform::SampleAndHold => {
                // Значение обновляется на каждом периоде
                // Здесь просто возвращаем фазу как заглушку
                // Реальное значение хранится в состоянии автомата
                phase
            }
            
            LfoWaveform::RandomWalk => {
                // Непрерывное случайное блуждание
                // Здесь просто возвращаем фазу как заглушку
                phase
            }
        }
    }
}

/// Состояние LFO
#[derive(Debug, Clone)]
pub struct LfoState {
    /// Текущая фаза (0.0 - 1.0)
    pub phase: f64,
    /// Текущее значение (для S&H и RandomWalk)
    pub value: f64,
    /// Счётчик семплов для S&H
    pub hold_counter: usize,
    /// Случайное зерно
    pub rng_state: u64,
}

/// Действия для LFO
#[derive(Debug, Clone, Default)]
pub enum LfoAction {
    /// Нет действия
    #[default]
    None,
    /// Сбросить фазу в 0
    Reset,
    /// Синхронизировать с внешним тактом
    Sync,
    /// Установить новую частоту
    SetFrequency(f64),
    /// Установить новую амплитуду
    SetAmplitude(f64),
    /// Установить новое смещение
    SetOffset(f64),
}

/// LFO автомат
#[derive(Debug, Clone)]
pub struct LfoAutomaton {
    /// Имя автомата
    name: String,
    /// Частота (Hz)
    frequency: f64,
    /// Амплитуда
    amplitude: f64,
    /// Смещение
    offset: f64,
    /// Форма волны
    waveform: LfoWaveform,
    /// Диапазон выходных значений
    range: Range,
    /// Режим синхронизации
    sync_mode: SyncMode,
    /// Ширина импульса (для Pulse)
    pulse_width: f64,
    /// Скорость случайного блуждания
    walk_rate: f64,
}

impl LfoAutomaton {
    /// Создать новый LFO
    pub fn new(
        name: &str,
        frequency: f64,
        amplitude: f64,
        offset: f64,
        waveform: LfoWaveform,
    ) -> Self {
        Self {
            name: name.to_string(),
            frequency: frequency.max(0.001),
            amplitude,
            offset,
            waveform,
            range: Range::bipolar(),
            sync_mode: SyncMode::Free,
            pulse_width: 0.5,
            walk_rate: 0.1,
        }
    }
    
    /// Установить диапазон
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }
    
    /// Установить режим синхронизации
    pub fn with_sync_mode(mut self, mode: SyncMode) -> Self {
        self.sync_mode = mode;
        self
    }
    
    /// Установить ширину импульса
    pub fn with_pulse_width(mut self, width: f64) -> Self {
        self.pulse_width = width.clamp(0.01, 0.99);
        self
    }
    
    /// Установить скорость случайного блуждания
    pub fn with_walk_rate(mut self, rate: f64) -> Self {
        self.walk_rate = rate.max(0.0);
        self
    }
    
    /// Обновить фазу
    fn update_phase(&self, state: &mut LfoState, dt: f64) {
        match self.sync_mode {
            SyncMode::Free => {
                state.phase += self.frequency * dt;
                if state.phase >= 1.0 {
                    state.phase -= 1.0;
                    // Обновляем S&H при завершении периода
                    if let LfoWaveform::SampleAndHold = self.waveform {
                        state.value = self.random(&mut state.rng_state);
                    }
                }
            }
            SyncMode::Sync => {
                // Фаза управляется внешними синхроимпульсами
                // Ничего не делаем здесь
            }
            SyncMode::OneShot => {
                if state.phase < 1.0 {
                    state.phase += self.frequency * dt;
                }
            }
        }
    }
    
    /// Генератор случайных чисел (Xorshift)
    fn random(&self, state: &mut u64) -> f64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        
        (x as f64 / u64::MAX as f64) * 2.0 - 1.0
    }
    
    /// Обновить случайное блуждание
    fn update_random_walk(&self, state: &mut LfoState, dt: f64) {
        let step = (self.random(&mut state.rng_state) - 0.5) * self.walk_rate * dt * 100.0;
        state.value = (state.value + step).clamp(-1.0, 1.0);
    }
}

impl Automaton for LfoAutomaton {
    type State = LfoState;
    type Action = LfoAction;
    
    fn step(
        &self,
        time: Time,
        action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>, Option<Self::Action>) {
        let mut new_state = state.clone();
        let mut next_action = None;
        
        // Обрабатываем действия
        match action {
            LfoAction::Reset => {
                new_state.phase = 0.0;
                new_state.value = 0.0;
            }
            LfoAction::Sync => {
                new_state.phase = 0.0;
            }
            LfoAction::SetFrequency(freq) => {
                let mut new_automaton = self.clone();
                new_automaton.frequency = freq.max(0.001);
                // В реальном коде нужно обновить автомат
            }
            LfoAction::SetAmplitude(amp) => {
                let mut new_automaton = self.clone();
                new_automaton.amplitude = amp;
            }
            LfoAction::SetOffset(off) => {
                let mut new_automaton = self.clone();
                new_automaton.offset = off;
            }
            LfoAction::None => {}
        }
        
        // Вычисляем dt (разницу во времени)
        // В реальном коде нужно хранить предыдущее время
        let dt = 0.01; // Заглушка
        
        // Обновляем фазу
        self.update_phase(&mut new_state, dt);
        
        // Обновляем случайное блуждание
        if let LfoWaveform::RandomWalk = self.waveform {
            self.update_random_walk(&mut new_state, dt);
        }
        
        // Вычисляем значение
        let raw_value = match self.waveform {
            LfoWaveform::SampleAndHold => new_state.value,
            LfoWaveform::RandomWalk => new_state.value,
            _ => self.waveform.evaluate(new_state.phase, Some(self.pulse_width)),
        };
        
        let value = raw_value * self.amplitude + self.offset;
        let clamped = self.range.clamp(value);
        
        (new_state, Some(clamped), next_action)
    }
    
    fn initial_state(&self) -> Self::State {
        LfoState {
            phase: 0.0,
            value: 0.0,
            hold_counter: 0,
            rng_state: 123456789,
        }
    }
    
    fn name(&self) -> &str {
        &self.name
    }
    
    fn extract_value(&self, state: &Self::State) -> f64 {
        let raw = match self.waveform {
            LfoWaveform::SampleAndHold => state.value,
            LfoWaveform::RandomWalk => state.value,
            _ => self.waveform.evaluate(state.phase, Some(self.pulse_width)),
        };
        self.range.clamp(raw * self.amplitude + self.offset)
    }
}

// =============================================================================
// Тесты
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;
    
    #[test]
    fn test_sine_lfo() {
        let lfo = LfoAutomaton::new("Sine", 1.0, 1.0, 0.0, LfoWaveform::Sine);
        let state = lfo.initial_state();
        
        let (state, value, _) = lfo.step(0.0, LfoAction::None, &state);
        assert!(approx_eq!(f64, value.unwrap(), 0.0, epsilon = 0.01));
        
        let (state, value, _) = lfo.step(0.25, LfoAction::None, &state);
        assert!(approx_eq!(f64, value.unwrap(), 1.0, epsilon = 0.01));
    }
    
    #[test]
    fn test_reset_action() {
        let lfo = LfoAutomaton::new("Test", 1.0, 1.0, 0.0, LfoWaveform::Sine);
        let mut state = lfo.initial_state();
        state.phase = 0.5;
        
        let (new_state, _, _) = lfo.step(1.0, LfoAction::Reset, &state);
        assert!(approx_eq!(f64, new_state.phase, 0.0, epsilon = 0.01));
    }
}