//! # Random processes
//!
//! Automata for generating random and pseudo-random sequences:
//! - Random Walk
//! - Chaos (deterministic chaos)
//! - Noise (white, pink, brown noise)

use crate::engine::{Automaton, NoAction, Range, Time};
use rill_core::traits::ParamValue;

/// Type of random process
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RandomType {
    /// Random walk
    Walk,
    /// Logistic map (chaos)
    Logistic,
    /// Hénon map
    Henon,
    /// Lorenz system
    Lorenz,
    /// White noise
    WhiteNoise,
    /// Pink noise (1/f)
    PinkNoise,
    /// Brown noise (1/f²)
    BrownNoise,
}

/// Random process automaton
#[derive(Debug, Clone)]
pub struct RandomAutomaton {
    /// Automaton name
    name: String,
    /// Type of random process
    rng_type: RandomType,
    /// Output value range
    range: Range,
    /// Rate of change (for Walk)
    rate: f64,
    /// Chaos parameters
    chaos_params: (f64, f64, f64), // r, a, b etc.
    /// Update rate (for noise)
    update_rate: f64,
}

impl RandomAutomaton {
    /// Create a new Random Walk
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

    /// Create logistic map (chaos)
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

    /// Create Hénon map
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

    /// Create white noise generator
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

    /// Set the range
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

    /// Random number in the range [0, 1)
    fn random_f64(&self, state: &mut u64) -> f64 {
        self.xorshift(state) as f64 / u64::MAX as f64
    }
}

impl Automaton for RandomAutomaton {
    type Internal = (u64, f64, f64); // rng_state, last_value, last_update_time
    type Action = NoAction;

    fn step(
        &self,
        internal: &mut Self::Internal,
        _current: &ParamValue,
        time: Time,
        _action: &Self::Action,
    ) -> ParamValue {
        let (rng_state, last_value, last_update_time) = internal;
        let period = if self.update_rate > 0.0 {
            1.0 / self.update_rate
        } else {
            0.0
        };

        if period > 0.0 && time - *last_update_time < period {
            return ParamValue::Float(*last_value as f32);
        }
        *last_update_time = time;

        let new_value = match self.rng_type {
            RandomType::Walk => {
                let step = (self.random_f64(rng_state) - 0.5) * 2.0 * self.rate;
                (*last_value + step).clamp(-1.0, 1.0)
            }
            RandomType::Logistic => {
                let r = self.chaos_params.0;
                r * *last_value * (1.0 - *last_value)
            }
            RandomType::Henon => {
                let a = self.chaos_params.0;
                let x = *last_value;
                let y = *last_value - x;
                1.0 - a * x * x + y
            }
            RandomType::WhiteNoise => self.random_f64(rng_state) * 2.0 - 1.0,
            _ => *last_value,
        };

        *last_value = new_value;
        let value = self.range.clamp(new_value);
        ParamValue::Float(value as f32)
    }

    fn initial_internal(&self) -> Self::Internal {
        let initial_value = match self.rng_type {
            RandomType::Logistic => 0.5,
            RandomType::Henon => 0.0,
            _ => 0.0,
        };
        (123456789, initial_value, 0.0)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_walk() {
        let walk = RandomAutomaton::walk("Walk", 0.1);
        let mut internal = walk.initial_internal();
        let current = ParamValue::Float(0.0);

        let value = walk.step(&mut internal, &current, 0.01, &NoAction);
        let val = value.as_f32().unwrap();
        assert!((-1.0..=1.0).contains(&val));
    }

    #[test]
    fn test_logistic() {
        let logistic = RandomAutomaton::logistic("Logistic", 3.8);
        let mut internal = logistic.initial_internal();
        let current = ParamValue::Float(0.0);

        let value = logistic.step(&mut internal, &current, 0.0, &NoAction);
        let val = value.as_f32().unwrap();
        assert!((0.0..=1.0).contains(&val));
    }
}
