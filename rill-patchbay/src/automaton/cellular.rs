//! # Клеточные автоматы
//!
//! Генерация сигналов на основе клеточных автоматов.
//! Поддерживаются 1D и 2D клеточные автоматы с различными правилами.

use crate::engine::{Automaton, NoAction, Range, Time};

/// Тип клеточного автомата
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellularType {
    /// 1D клеточный автомат (правило Вольфрама)
    OneDimensional,
    /// 2D клеточный автомат (Game of Life)
    TwoDimensional,
    /// Циклический клеточный автомат
    Cyclic,
}

/// Состояние клеточного автомата
#[derive(Debug, Clone)]
pub struct CellularState {
    /// Текущее поколение
    pub generation: Vec<u8>,
    /// Следующее поколение
    pub next_generation: Vec<u8>,
    /// Ширина (для 2D)
    pub width: usize,
    /// Высота (для 2D)
    pub height: usize,
    /// Текущее значение (для вывода)
    pub current_value: f64,
    /// Счётчик шагов
    pub step: usize,
}

/// Клеточный автомат
#[derive(Debug, Clone)]
pub struct CellularAutomaton {
    /// Имя автомата
    name: String,
    /// Тип автомата
    cell_type: CellularType,
    /// Правило (для 1D: 0-255)
    rule: u8,
    /// Размер (количество клеток для 1D, ширина/высота для 2D)
    size: usize,
    /// Способ преобразования поколения в выходной сигнал
    output_mode: OutputMode,
    /// Диапазон выходных значений
    range: Range,
    /// Случайное зерно для инициализации
    rng_state: u64,
}

/// Режим преобразования поколения в сигнал
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    /// Использовать центральную клетку
    Center,
    /// Использовать сумму всех клеток (нормализованную)
    Sum,
    /// Использовать плотность активных клеток
    Density,
    /// Использовать конкретный индекс
    Index(usize),
}

impl CellularAutomaton {
    /// Создать новый 1D клеточный автомат
    pub fn one_dimensional(name: &str, rule: u8, size: usize) -> Self {
        Self {
            name: name.to_string(),
            cell_type: CellularType::OneDimensional,
            rule,
            size,
            output_mode: OutputMode::Center,
            range: Range::unipolar(),
            rng_state: 123456789,
        }
    }

    /// Создать новый Game of Life
    pub fn game_of_life(name: &str, width: usize, height: usize) -> Self {
        Self {
            name: name.to_string(),
            cell_type: CellularType::TwoDimensional,
            rule: 0,
            size: width * height,
            output_mode: OutputMode::Density,
            range: Range::unipolar(),
            rng_state: 123456789,
        }
    }

    /// Установить режим вывода
    pub fn with_output_mode(mut self, mode: OutputMode) -> Self {
        self.output_mode = mode;
        self
    }

    /// Установить диапазон
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }

    /// Инициализировать случайное состояние
    fn random_initial(&self, _width: usize, _height: usize, rng: &mut u64) -> Vec<u8> {
        let mut gen = Vec::with_capacity(self.size);
        for _ in 0..self.size {
            gen.push(if self.random_bit(rng) { 1 } else { 0 });
        }
        gen
    }

    /// Случайный бит
    fn random_bit(&self, rng: &mut u64) -> bool {
        let mut x = *rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *rng = x;
        (x & 1) == 1
    }

    /// Применить правило Вольфрама (1D)
    fn apply_rule_1d(&self, generation: &[u8]) -> Vec<u8> {
        let mut next = vec![0; generation.len()];

        for i in 0..generation.len() {
            let left = if i > 0 {
                generation[i - 1]
            } else {
                generation[generation.len() - 1]
            };
            let center = generation[i];
            let right = if i < generation.len() - 1 {
                generation[i + 1]
            } else {
                generation[0]
            };

            let pattern = (left << 2) | (center << 1) | right;
            let bit = (self.rule >> pattern) & 1;
            next[i] = bit;
        }

        next
    }

    /// Применить правило Game of Life (2D)
    fn apply_rule_gol(&self, generation: &[u8], width: usize, height: usize) -> Vec<u8> {
        let mut next = vec![0; generation.len()];

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let cell = generation[idx];

                // Считаем живых соседей (8 направлений)
                let mut neighbors = 0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        if dx == 0 && dy == 0 {
                            continue;
                        }

                        let nx = (x as i32 + dx + width as i32) % width as i32;
                        let ny = (y as i32 + dy + height as i32) % height as i32;
                        let nidx = (ny * width as i32 + nx) as usize;

                        if generation[nidx] == 1 {
                            neighbors += 1;
                        }
                    }
                }

                // Правила Game of Life
                next[idx] = match (cell, neighbors) {
                    (1, 2) | (1, 3) => 1, // Выживание
                    (0, 3) => 1,          // Рождение
                    _ => 0,               // Смерть
                };
            }
        }

        next
    }

    /// Вычислить выходное значение
    fn compute_output(&self, generation: &[u8], _state: &CellularState) -> f64 {
        match self.output_mode {
            OutputMode::Center => {
                let idx = self.size / 2;
                generation[idx] as f64
            }

            OutputMode::Sum => {
                let sum: usize = generation.iter().map(|&c| c as usize).sum();
                sum as f64 / self.size as f64
            }

            OutputMode::Density => {
                let sum: usize = generation.iter().map(|&c| c as usize).sum();
                sum as f64 / self.size as f64
            }

            OutputMode::Index(idx) => {
                if idx < generation.len() {
                    generation[idx] as f64
                } else {
                    0.0
                }
            }
        }
    }
}

impl Automaton for CellularAutomaton {
    type State = CellularState;
    type Action = NoAction;

    fn step(
        &self,
        _time: Time,
        _action: &Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>) {
        let mut new_state = state.clone();

        new_state.next_generation = match self.cell_type {
            CellularType::OneDimensional => self.apply_rule_1d(&state.generation),
            CellularType::TwoDimensional => {
                self.apply_rule_gol(&state.generation, state.width, state.height)
            }
            CellularType::Cyclic => state.generation.clone(),
        };

        std::mem::swap(&mut new_state.generation, &mut new_state.next_generation);
        new_state.step += 1;

        let raw = self.compute_output(&new_state.generation, &new_state);
        let value = self.range.clamp(raw);

        (new_state, Some(value))
    }

    fn initial_state(&self) -> Self::State {
        let mut rng = self.rng_state;
        let generation = self.random_initial(self.size, 1, &mut rng);

        CellularState {
            generation,
            next_generation: vec![0; self.size],
            width: self.size,
            height: 1,
            current_value: 0.0,
            step: 0,
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn extract_value(&self, state: &Self::State) -> f64 {
        self.compute_output(&state.generation, state)
    }
}

/// Предустановленные правила для 1D клеточных автоматов
pub mod rules {
    /// Правило 30: хаотическое поведение
    pub const RULE_30: u8 = 30;
    /// Правило 90: фрактальное (треугольник Серпинского)
    pub const RULE_90: u8 = 90;
    /// Правило 110: универсальное (Тьюринг-полное)
    pub const RULE_110: u8 = 110;
    /// Правило 184: дорожное движение
    pub const RULE_184: u8 = 184;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_30() {
        let ca = CellularAutomaton::one_dimensional("Rule 30", rules::RULE_30, 31);
        let state = ca.initial_state();

        let (_state, value) = ca.step(0.0, &NoAction, &state);
        assert!(value.is_some());
    }
}
