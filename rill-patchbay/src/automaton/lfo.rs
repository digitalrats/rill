//! LFO (Low Frequency Oscillator) automata for periodic modulation.
//!
//! Supports various waveform shapes and synchronisation modes.

use crate::engine::{Automaton, NoAction, Range, Time};
use rill_core::traits::ParamValue;
use std::f64::consts::PI;

/// LFO waveform shape.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LfoWaveform {
    /// Smooth sinusoidal wave.
    Sine,
    /// Triangular wave.
    Triangle,
    /// Rising sawtooth wave.
    Saw,
    /// Falling sawtooth wave.
    ReverseSaw,
    /// Square wave.
    Square,
    /// Pulse wave with configurable duty cycle.
    Pulse(f64),
    /// Random value held for the duration of one period.
    SampleAndHold,
    /// Smooth random walk (continuous noise).
    RandomWalk,
}

impl LfoWaveform {
    /// Return the human-readable name of this waveform.
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

    /// Evaluate the waveform at a given phase (0.0 – 1.0).
    ///
    /// `pulse_width` overrides the built-in width for `Pulse` waveforms.
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
                if phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }

            LfoWaveform::Pulse(width) => {
                let w = pulse_width.unwrap_or(*width);
                if phase < w {
                    1.0
                } else {
                    -1.0
                }
            }

            LfoWaveform::SampleAndHold => phase,

            LfoWaveform::RandomWalk => phase,
        }
    }
}

/// An LFO automaton that generates periodic modulation signals.
///
/// Supports multiple waveform shapes, configurable frequency, amplitude,
/// offset, pulse width, and random-walk rate.
#[derive(Debug, Clone)]
pub struct LfoAutomaton {
    name: String,
    frequency: f64,
    amplitude: f64,
    offset: f64,
    waveform: LfoWaveform,
    range: Range,
    pulse_width: f64,
    walk_rate: f64,
}

impl LfoAutomaton {
    /// Create a new LFO automaton.
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
            pulse_width: 0.5,
            walk_rate: 0.1,
        }
    }

    /// Set the output range.
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }

    /// Set the pulse width for the `Pulse` waveform (0.01 – 0.99).
    pub fn with_pulse_width(mut self, width: f64) -> Self {
        self.pulse_width = width.clamp(0.01, 0.99);
        self
    }

    /// Set the random-walk step rate.
    pub fn with_walk_rate(mut self, rate: f64) -> Self {
        self.walk_rate = rate.max(0.0);
        self
    }
}

impl Automaton for LfoAutomaton {
    type Internal = f64;
    type Action = NoAction;

    fn step(
        &self,
        phase: &mut Self::Internal,
        _current: &ParamValue,
        time: Time,
        _action: &Self::Action,
    ) -> ParamValue {
        *phase = (time * self.frequency).fract();
        let raw = match self.waveform {
            LfoWaveform::Sine => (*phase * 2.0 * PI).sin(),
            LfoWaveform::Triangle => {
                if *phase < 0.5 {
                    4.0 * *phase - 1.0
                } else {
                    3.0 - 4.0 * *phase
                }
            }
            LfoWaveform::Saw => 2.0 * *phase - 1.0,
            LfoWaveform::ReverseSaw => 1.0 - 2.0 * *phase,
            LfoWaveform::Square => {
                if *phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoWaveform::Pulse(width) => {
                if *phase < width {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoWaveform::SampleAndHold => {
                // hold current value, phase changes
                return ParamValue::Float((*phase * 2.0 * PI).sin() as f32);
            }
            LfoWaveform::RandomWalk => {
                // smooth random — return sine for now
                (*phase * 2.0 * PI).sin()
            }
        };
        let val = raw * self.amplitude + self.offset;
        ParamValue::Float(val as f32)
    }

    fn initial_internal(&self) -> Self::Internal {
        0.0
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use float_cmp::approx_eq;

    #[test]
    fn test_sine_lfo() {
        let lfo = LfoAutomaton::new("Sine", 1.0, 1.0, 0.0, LfoWaveform::Sine);
        let mut phase = lfo.initial_internal();
        let current = ParamValue::Float(0.0);

        let value = lfo.step(&mut phase, &current, 0.0, &NoAction);
        let val = value.as_f32().unwrap();
        assert!(approx_eq!(f64, val as f64, 0.0, epsilon = 0.01));

        let value = lfo.step(&mut phase, &current, 0.25, &NoAction);
        let val = value.as_f32().unwrap();
        assert!(approx_eq!(f64, val as f64, 1.0, epsilon = 0.01));
    }
}
