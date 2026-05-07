//! # Sequencers
//!
//! Automata for generating rhythmic patterns and sequences
//! of values over time.

use crate::engine::{Automaton, NoAction, Range, Time};
use std::collections::VecDeque;

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

/// Sequencer state
#[derive(Debug, Clone)]
pub struct SequencerState {
    /// Current step index
    pub current_step: usize,
    /// Start time of the current step
    pub step_start_time: Time,
    /// Value at the current step
    pub current_value: f64,
    /// Target value (for interpolation)
    pub target_value: f64,
    /// Direction (for PingPong)
    pub direction: i8,
    /// History of recent steps (for Brownian)
    pub history: VecDeque<usize>,
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
    /// Random seed
    rng_state: u64,
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
            rng_state: 123456789,
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

    /// Select the next step
    fn next_step(&self, state: &SequencerState) -> usize {
        match self.mode {
            PlayMode::OneShot => {
                if state.current_step < self.steps.len() - 1 {
                    state.current_step + 1
                } else {
                    state.current_step
                }
            }

            PlayMode::Loop => (state.current_step + 1) % self.steps.len(),

            PlayMode::PingPong => {
                let next = state.current_step as i32 + state.direction as i32;
                if next < 0 {
                    1
                } else if next >= self.steps.len() as i32 {
                    self.steps.len() - 2
                } else {
                    next as usize
                }
            }

            PlayMode::Random => self.random_index(&mut self.rng_state.clone()),

            PlayMode::Brownian => self.brownian_next(state, &mut self.rng_state.clone()),
        }
    }

    /// Random index
    fn random_index(&self, rng: &mut u64) -> usize {
        let mut x = *rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *rng = x;

        (x as usize) % self.steps.len()
    }

    /// Next step for Brownian motion
    fn brownian_next(&self, state: &SequencerState, rng: &mut u64) -> usize {
        let mut candidates = Vec::new();

        // Can stay in place
        candidates.push(state.current_step);

        // Or move to neighbors
        if state.current_step > 0 {
            candidates.push(state.current_step - 1);
        }
        if state.current_step < self.steps.len() - 1 {
            candidates.push(state.current_step + 1);
        }

        let idx = self.random_index(rng) % candidates.len();
        candidates[idx]
    }
}

impl Automaton for SequencerAutomaton {
    type State = SequencerState;
    type Action = NoAction;

    fn step(
        &self,
        time: Time,
        _action: &Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>) {
        let mut new_state = state.clone();

        // Check if it's time to advance to the next step
        let current_step = &self.steps[new_state.current_step];
        let step_dur = self.step_duration(current_step);
        let elapsed = time - new_state.step_start_time;

        if elapsed >= step_dur {
            // Advance to the next step
            let next = self.next_step(&new_state);
            new_state.current_step = next;
            new_state.step_start_time = time;
            new_state.current_value = self.steps[next].value;
            new_state.target_value = self.steps[next].value;

            if let PlayMode::PingPong = self.mode {
                if next == 0 {
                    new_state.direction = 1;
                } else if next == self.steps.len() - 1 {
                    new_state.direction = -1;
                }
            }

            if let PlayMode::Brownian = self.mode {
                new_state.history.push_back(next);
                if new_state.history.len() > 10 {
                    new_state.history.pop_front();
                }
            }
        } else if self.interpolate && step_dur > 0.0 {
            let t = elapsed / step_dur;
            let next_idx = (new_state.current_step + 1) % self.steps.len();
            let next_val = self.steps[next_idx].value;

            let curve = current_step.curve.unwrap_or(1.0);
            let tt = t.powf(curve);

            new_state.current_value = current_step.value * (1.0 - tt) + next_val * tt;
        }

        let value = self.range.denormalize(new_state.current_value);

        (new_state, Some(value))
    }

    fn initial_state(&self) -> Self::State {
        SequencerState {
            current_step: 0,
            step_start_time: 0.0,
            current_value: self.steps[0].value,
            target_value: self.steps[0].value,
            direction: 1,
            history: VecDeque::with_capacity(10),
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn extract_value(&self, state: &Self::State) -> f64 {
        self.range.denormalize(state.current_value)
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
        let state = seq.initial_state();

        assert_eq!(state.current_value, 0.0);

        let (new_state, _value) = seq.step(0.6, &NoAction, &state);
        assert_eq!(new_state.current_step, 1);
    }
}
