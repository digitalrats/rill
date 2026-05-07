//! # Cellular automata
//!
//! Signal generation based on cellular automata.
//! Supports 1D and 2D cellular automata with various rules.

use crate::engine::{Automaton, NoAction, Range, Time};

/// Type of cellular automaton
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellularType {
    /// 1D cellular automaton (Wolfram rule)
    OneDimensional,
    /// 2D cellular automaton (Game of Life)
    TwoDimensional,
    /// Cyclic cellular automaton
    Cyclic,
}

/// Cellular automaton state
#[derive(Debug, Clone)]
pub struct CellularState {
    /// Current generation
    pub generation: Vec<u8>,
    /// Next generation
    pub next_generation: Vec<u8>,
    /// Width (for 2D)
    pub width: usize,
    /// Height (for 2D)
    pub height: usize,
    /// Current value (for output)
    pub current_value: f64,
    /// Step counter
    pub step: usize,
}

/// Cellular automaton
#[derive(Debug, Clone)]
pub struct CellularAutomaton {
    /// Automaton name
    name: String,
    /// Type of automaton
    cell_type: CellularType,
    /// Rule (for 1D: 0-255)
    rule: u8,
    /// Size (number of cells for 1D, width/height for 2D)
    size: usize,
    /// Method for converting generation to output signal
    output_mode: OutputMode,
    /// Output value range
    range: Range,
    /// Random seed for initialization
    rng_state: u64,
}

/// Generation-to-signal conversion mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutputMode {
    /// Use the center cell
    Center,
    /// Use the sum of all cells (normalized)
    Sum,
    /// Use density of active cells
    Density,
    /// Use a specific index
    Index(usize),
}

impl CellularAutomaton {
    /// Create a new 1D cellular automaton
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

    /// Create a new Game of Life
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

    /// Set the output mode
    pub fn with_output_mode(mut self, mode: OutputMode) -> Self {
        self.output_mode = mode;
        self
    }

    /// Set the range
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }

    /// Initialize random state
    fn random_initial(&self, _width: usize, _height: usize, rng: &mut u64) -> Vec<u8> {
        let mut gen = Vec::with_capacity(self.size);
        for _ in 0..self.size {
            gen.push(if self.random_bit(rng) { 1 } else { 0 });
        }
        gen
    }

    /// Random bit
    fn random_bit(&self, rng: &mut u64) -> bool {
        let mut x = *rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *rng = x;
        (x & 1) == 1
    }

    /// Apply Wolfram rule (1D)
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

    /// Apply Game of Life rule (2D)
    fn apply_rule_gol(&self, generation: &[u8], width: usize, height: usize) -> Vec<u8> {
        let mut next = vec![0; generation.len()];

        for y in 0..height {
            for x in 0..width {
                let idx = y * width + x;
                let cell = generation[idx];

                // Count live neighbors (8 directions)
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

                // Game of Life rules
                next[idx] = match (cell, neighbors) {
                    (1, 2) | (1, 3) => 1, // Survival
                    (0, 3) => 1,          // Birth
                    _ => 0,               // Death
                };
            }
        }

        next
    }

    /// Compute the output value
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

/// Preset rules for 1D cellular automata
pub mod rules {
    /// Rule 30: chaotic behavior
    pub const RULE_30: u8 = 30;
    /// Rule 90: fractal (Sierpinski triangle)
    pub const RULE_90: u8 = 90;
    /// Rule 110: universal (Turing-complete)
    pub const RULE_110: u8 = 110;
    /// Rule 184: traffic flow
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
