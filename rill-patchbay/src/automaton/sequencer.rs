//! # Sequencers
//!
//! Automata for generating rhythmic patterns and sequences
//! of values over time.

use crate::engine::{Automaton, NoAction, Range, Time};
use rill_core::traits::ParamValue;

/// Sequencer step
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone)]
pub struct Step {
    /// Value (0.0 - 1.0)
    pub value: f64,
    /// Duration in beat fractions
    pub duration: f64,
    /// Transition curve to the next step
    #[cfg_attr(feature = "serde", serde(default))]
    pub curve: Option<f64>,
}

/// Sequencer playback mode
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayMode {
    /// One shot
    OneShot,
    /// Loop
    Loop,
    /// Ping-pong
    PingPong,
    /// Random
    Random,
    /// Brownian
    Brownian,
}

/// Sequencer automaton
#[derive(Debug, Clone)]
pub struct SequencerAutomaton {
    /// Automaton name
    name: String,
    /// Sequencer steps
    steps: Vec<Step>,
    /// Playback mode
    mode: PlayMode,
    /// Tempo (BPM)
    tempo: f64,
    /// Duration scale (1.0 = quarter note)
    duration_scale: f64,
    /// Whether to interpolate between steps
    interpolate: bool,
    /// Output value range
    range: Range,
}

impl SequencerAutomaton {
    /// Create a new sequencer
    pub fn new(name: &str, steps: Vec<Step>) -> Self {
        Self {
            name: name.to_string(),
            steps,
            mode: PlayMode::Loop,
            tempo: 120.0,
            duration_scale: 1.0,
            interpolate: false,
            range: Range::unipolar(),
        }
    }

    /// Set playback mode
    pub fn with_mode(mut self, mode: PlayMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set tempo
    pub fn with_tempo(mut self, bpm: f64) -> Self {
        self.tempo = bpm.max(1.0);
        self
    }

    /// Enable/disable interpolation
    pub fn with_interpolation(mut self, interpolate: bool) -> Self {
        self.interpolate = interpolate;
        self
    }

    /// Set the range
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }

    /// Get step duration in seconds
    fn step_duration(&self, step: &Step) -> f64 {
        step.duration * 60.0 / self.tempo * 4.0 * self.duration_scale
    }

    /// Xorshift PRNG
    fn xorshift(&self, rng: &mut u64) -> u64 {
        let mut x = *rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *rng = x;
        x
    }

    /// Random index
    fn random_index(&self, rng: &mut u64) -> usize {
        let x = self.xorshift(rng);
        (x as usize) % self.steps.len()
    }

    /// Select the next step based on mode and current state
    fn next_step(&self, current_step: usize, direction: i8, rng_state: &mut u64) -> (usize, i8) {
        match self.mode {
            PlayMode::OneShot => {
                if current_step < self.steps.len() - 1 {
                    (current_step + 1, direction)
                } else {
                    (current_step, direction)
                }
            }

            PlayMode::Loop => ((current_step + 1) % self.steps.len(), direction),

            PlayMode::PingPong => {
                let next = current_step as i32 + direction as i32;
                if next < 0 {
                    (1, 1)
                } else if next >= self.steps.len() as i32 {
                    (self.steps.len() - 2, -1)
                } else {
                    (next as usize, direction)
                }
            }

            PlayMode::Random => (self.random_index(rng_state), direction),

            PlayMode::Brownian => {
                let mut candidates = vec![current_step];
                if current_step > 0 {
                    candidates.push(current_step - 1);
                }
                if current_step < self.steps.len() - 1 {
                    candidates.push(current_step + 1);
                }
                let idx = self.random_index(rng_state) % candidates.len();
                (candidates[idx], direction)
            }
        }
    }
}

impl Automaton for SequencerAutomaton {
    type Internal = (usize, f64, i8, u64);
    type Action = NoAction;

    fn step(
        &self,
        internal: &mut Self::Internal,
        _current: &ParamValue,
        time: Time,
        _action: &Self::Action,
    ) -> ParamValue {
        let (current_step, step_start_time, direction, rng_state) = *internal;

        if self.steps.is_empty() {
            return ParamValue::Float(0.0);
        }

        let current_step_data = &self.steps[current_step];
        let step_dur = self.step_duration(current_step_data);
        let elapsed = time - step_start_time;

        let (new_step, new_start_time, new_direction, new_rng, value) = if elapsed >= step_dur {
            let (next, new_dir) = self.next_step(current_step, direction, &mut rng_state.clone());
            (next, time, new_dir, rng_state, self.steps[next].value)
        } else if self.interpolate && step_dur > 0.0 {
            let t = elapsed / step_dur;
            let next_idx = (current_step + 1) % self.steps.len();
            let next_val = self.steps[next_idx].value;
            let curve = current_step_data.curve.unwrap_or(1.0);
            let tt = t.powf(curve);
            let v = current_step_data.value * (1.0 - tt) + next_val * tt;
            (current_step, step_start_time, direction, rng_state, v)
        } else {
            (
                current_step,
                step_start_time,
                direction,
                rng_state,
                current_step_data.value,
            )
        };

        *internal = (new_step, new_start_time, new_direction, new_rng);
        let out = self.range.denormalize(value);
        ParamValue::Float(out as f32)
    }

    fn initial_internal(&self) -> Self::Internal {
        (0, 0.0, 1, 123456789)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Create a simple sequence of equal steps
pub fn simple_sequence(values: Vec<f64>, duration: f64) -> Vec<Step> {
    values
        .into_iter()
        .map(|v| Step {
            value: v.clamp(0.0, 1.0),
            duration,
            curve: None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sequencer() {
        let steps = simple_sequence(vec![0.0, 0.5, 1.0, 0.5], 0.25);
        let seq = SequencerAutomaton::new("Test", steps);
        let mut internal = seq.initial_internal();
        let current = ParamValue::Float(0.0);

        assert_eq!(internal.0, 0);

        let _value = seq.step(&mut internal, &current, 0.6, &NoAction);
        assert_eq!(internal.0, 1);
    }
}
