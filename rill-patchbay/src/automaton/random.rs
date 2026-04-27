//! # Случайные процессы
//!
//! Автоматы для генерации случайных и псевдослучайных последовательностей:
//! - Random Walk (случайное блуждание)
//! - Chaos (детерминированный хаос)
//! - Noise (белый, розовый, коричневый шум)

use crate::control::{Automaton, NoAction, Range, Time};

/// Тип случайного процесса
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RandomType {
    /// Случайное блуждание
    Walk,
    /// Логистическое отображение (хаос)
    Logistic,
    /// Отображение Эно
    Henon,
    /// Отображение Лоренца
    Lorenz,
    /// Белый шум
    WhiteNoise,
    /// Розовый шум (1/f)
    PinkNoise,
    /// Коричневый шум (1/f²)
    BrownNoise,
}

/// Состояние случайного процесса
#[derive(Debug, Clone)]
pub struct RandomState {
    /// Текущее значение
    pub value: f64,
    /// Внутреннее состояние RNG
    pub rng_state: u64,
    /// Дополнительные состояния для сложных процессов
    pub extra: Vec<f64>,
    /// Время последнего обновления
    pub last_time: Time,
}

/// Автомат случайного процесса
#[derive(Debug, Clone)]
pub struct RandomAutomaton {
    /// Имя автомата
    name: String,
    /// Тип случайного процесса
    rng_type: RandomType,
    /// Диапазон выходных значений
    range: Range,
    /// Скорость изменения (для Walk)
    rate: f64,
    /// Параметры хаоса
    chaos_params: (f64, f64, f64), // r, a, b и т.д.
    /// Частота обновления (для шума)
    update_rate: f64,
}

impl RandomAutomaton {
    /// Создать новый Random Walk
    pub fn walk(name: &str, rate: f64) -> Self {
        Self {
            name: name.to_string(),
            rng_type: RandomType::Walk,
            range: Range::bipolar(),
            rate: rate.max(0.0),
            chaos_params: (0.0, 0.0, 0.0),
            update_rate: 0.0,
        }
    }

    /// Создать логистическое отображение (хаос)
    pub fn logistic(name: &str, r: f64) -> Self {
        Self {
            name: name.to_string(),
            rng_type: RandomType::Logistic,
            range: Range::unipolar(),
            rate: 0.0,
            chaos_params: (r.clamp(3.0, 4.0), 0.0, 0.0),
            update_rate: 0.0,
        }
    }

    /// Создать отображение Эно
    pub fn henon(name: &str, a: f64, b: f64) -> Self {
        Self {
            name: name.to_string(),
            rng_type: RandomType::Henon,
            range: Range::bipolar(),
            rate: 0.0,
            chaos_params: (a, b, 0.0),
            update_rate: 0.0,
        }
    }

    /// Создать генератор белого шума
    pub fn white_noise(name: &str, update_rate: f64) -> Self {
        Self {
            name: name.to_string(),
            rng_type: RandomType::WhiteNoise,
            range: Range::bipolar(),
            rate: 0.0,
            chaos_params: (0.0, 0.0, 0.0),
            update_rate: update_rate.max(1.0),
        }
    }

    /// Установить диапазон
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }

    /// Xorshift RNG
    fn xorshift(&self, state: &mut u64) -> u64 {
        let mut x = *state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *state = x;
        x
    }

    /// Случайное число в диапазоне [0, 1)
    fn random_f64(&self, state: &mut u64) -> f64 {
        self.xorshift(state) as f64 / u64::MAX as f64
    }

    /// Обновить Random Walk
    fn update_walk(&self, state: &mut RandomState, dt: Time) {
        let step = (self.random_f64(&mut state.rng_state) - 0.5) * 2.0 * self.rate * dt;
        state.value = (state.value + step).clamp(-1.0, 1.0);
    }

    /// Обновить логистическое отображение
    fn update_logistic(&self, state: &mut RandomState, _dt: Time) {
        let r = self.chaos_params.0;
        state.value = r * state.value * (1.0 - state.value);
    }

    /// Обновить отображение Эно
    fn update_henon(&self, state: &mut RandomState, _dt: Time) {
        let a = self.chaos_params.0;
        let b = self.chaos_params.1;

        if state.extra.is_empty() {
            state.extra.push(0.0);
        }

        let x = state.value;
        let y = state.extra[0];

        state.value = 1.0 - a * x * x + y;
        state.extra[0] = b * x;
    }

    /// Обновить белый шум
    fn update_white_noise(&self, state: &mut RandomState, dt: Time) {
        state.last_time += dt;
        if state.last_time >= 1.0 / self.update_rate {
            state.value = self.random_f64(&mut state.rng_state) * 2.0 - 1.0;
            state.last_time = 0.0;
        }
    }
}

impl Automaton for RandomAutomaton {
    type State = RandomState;
    type Action = NoAction;

    fn step(
        &self,
        time: Time,
        _action: &Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>) {
        let mut new_state = state.clone();
        let dt = time - state.last_time;

        match self.rng_type {
            RandomType::Walk => self.update_walk(&mut new_state, dt),
            RandomType::Logistic => self.update_logistic(&mut new_state, dt),
            RandomType::Henon => self.update_henon(&mut new_state, dt),
            RandomType::WhiteNoise => self.update_white_noise(&mut new_state, dt),
            _ => {}
        }

        new_state.last_time = time;
        let value = self.range.clamp(new_state.value);

        (new_state, Some(value))
    }

    fn initial_state(&self) -> Self::State {
        let rng = 123456789;
        let initial_value = match self.rng_type {
            RandomType::Logistic => 0.5,
            RandomType::Henon => 0.0,
            _ => 0.0,
        };

        RandomState {
            value: initial_value,
            rng_state: rng,
            extra: Vec::new(),
            last_time: 0.0,
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn extract_value(&self, state: &Self::State) -> f64 {
        self.range.clamp(state.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_walk() {
        let walk = RandomAutomaton::walk("Walk", 0.1);
        let state = walk.initial_state();

        let (_state, value) = walk.step(0.01, &NoAction, &state);
        assert!(value.unwrap() >= -1.0 && value.unwrap() <= 1.0);
    }

    #[test]
    fn test_logistic() {
        let logistic = RandomAutomaton::logistic("Logistic", 3.8);
        let state = logistic.initial_state();

        let (_state, value) = logistic.step(0.0, &NoAction, &state);
        assert!(value.unwrap() >= 0.0 && value.unwrap() <= 1.0);
    }
}
