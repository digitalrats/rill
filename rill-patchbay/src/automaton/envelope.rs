//! Envelope automata for amplitude, filter, and parameter modulation over time.
//!
//! Supports ADSR, AR, ASR, and AHDSR envelope types.

use crate::control::{Automaton, Range, Time};

/// Envelope shape type.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeType {
    /// Attack, Decay, Sustain, Release.
    ADSR,
    /// Attack, Release (suitable for percussion).
    AR,
    /// Attack, Sustain, Release (suitable for organ sounds).
    ASR,
    /// Attack, Hold, Decay, Sustain, Release.
    AHDSR,
}

/// Phase of the envelope lifecycle.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeStage {
    /// The signal is rising toward the peak level.
    Attack,
    /// The signal holds at the peak level (AHDSR only).
    Hold,
    /// The signal falls from the peak to the sustain level.
    Decay,
    /// The signal remains at the sustain level while gate is on.
    Sustain,
    /// The signal falls to zero after gate-off.
    Release,
    /// The envelope is silent.
    Off,
}

impl EnvelopeStage {
    /// Return the human-readable name of this stage.
    pub fn name(&self) -> &'static str {
        match self {
            EnvelopeStage::Attack => "Attack",
            EnvelopeStage::Hold => "Hold",
            EnvelopeStage::Decay => "Decay",
            EnvelopeStage::Sustain => "Sustain",
            EnvelopeStage::Release => "Release",
            EnvelopeStage::Off => "Off",
        }
    }
}

/// Runtime state of an envelope automaton.
///
/// Tracks the current stage, output level, timing information, and gate status.
#[derive(Debug, Clone)]
pub struct EnvelopeState {
    /// Current envelope phase.
    pub stage: EnvelopeStage,
    /// Current output level (0.0 – 1.0).
    pub level: f64,
    /// Time when the current stage began.
    pub stage_start_time: Time,
    /// Output level at the start of the current stage.
    pub stage_start_level: f64,
    /// Target level of the current stage.
    pub stage_target_level: f64,
    /// Duration of the current stage in seconds.
    pub stage_duration: f64,
    /// Whether the gate is on (triggered).
    pub gate: bool,
}

/// An envelope automaton that generates time-varying control signals.
///
/// The envelope progresses through stages (Attack, Decay, Sustain, Release, etc.)
/// and outputs a value that can be mapped to a parameter. The stage curve
/// controls the shape: 1.0 = linear, >1.0 = exponential, <1.0 = logarithmic.
#[derive(Debug, Clone)]
pub struct EnvelopeAutomaton {
    name: String,
    env_type: EnvelopeType,
    attack: f64,
    hold: f64,
    decay: f64,
    sustain: f64,
    release: f64,
    range: Range,
    curve: f64,
}

impl EnvelopeAutomaton {
    /// Create a new ADSR envelope.
    pub fn adsr(name: &str, attack: f64, decay: f64, sustain: f64, release: f64) -> Self {
        Self {
            name: name.to_string(),
            env_type: EnvelopeType::ADSR,
            attack: attack.max(0.001),
            hold: 0.0,
            decay: decay.max(0.001),
            sustain: sustain.clamp(0.0, 1.0),
            release: release.max(0.001),
            range: Range::unipolar(),
            curve: 1.0,
        }
    }

    /// Create a new AR envelope (suitable for percussion).
    pub fn ar(name: &str, attack: f64, release: f64) -> Self {
        Self {
            name: name.to_string(),
            env_type: EnvelopeType::AR,
            attack: attack.max(0.001),
            hold: 0.0,
            decay: 0.0,
            sustain: 0.0,
            release: release.max(0.001),
            range: Range::unipolar(),
            curve: 1.0,
        }
    }

    /// Create a new ASR envelope (suitable for organ sounds).
    pub fn asr(name: &str, attack: f64, sustain: f64, release: f64) -> Self {
        Self {
            name: name.to_string(),
            env_type: EnvelopeType::ASR,
            attack: attack.max(0.001),
            hold: 0.0,
            decay: 0.0,
            sustain: sustain.clamp(0.0, 1.0),
            release: release.max(0.001),
            range: Range::unipolar(),
            curve: 1.0,
        }
    }

    /// Create a new AHDSR envelope with an additional hold stage.
    pub fn ahdsr(
        name: &str,
        attack: f64,
        hold: f64,
        decay: f64,
        sustain: f64,
        release: f64,
    ) -> Self {
        Self {
            name: name.to_string(),
            env_type: EnvelopeType::AHDSR,
            attack: attack.max(0.001),
            hold: hold.max(0.001),
            decay: decay.max(0.001),
            sustain: sustain.clamp(0.0, 1.0),
            release: release.max(0.001),
            range: Range::unipolar(),
            curve: 1.0,
        }
    }

    /// Set the stage curve exponent (1.0 = linear).
    pub fn with_curve(mut self, curve: f64) -> Self {
        self.curve = curve.max(0.1);
        self
    }

    /// Set the output range.
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }

    /// Compute the curved interpolation factor.
    fn apply_curve(&self, t: f64) -> f64 {
        if self.curve == 1.0 {
            t
        } else {
            t.powf(self.curve)
        }
    }

    /// Advance the envelope stage based on elapsed time.
    fn update_stage(&self, state: &mut EnvelopeState, time: Time) {
        let elapsed = time - state.stage_start_time;

        match state.stage {
            EnvelopeStage::Attack => {
                if elapsed >= self.attack {
                    match self.env_type {
                        EnvelopeType::ADSR => {
                            state.stage = EnvelopeStage::Decay;
                            state.stage_start_time = time;
                            state.stage_start_level = 1.0;
                            state.stage_target_level = self.sustain;
                            state.stage_duration = self.decay;
                        }
                        EnvelopeType::AR => {
                            state.stage = EnvelopeStage::Release;
                            state.stage_start_time = time;
                            state.stage_start_level = 1.0;
                            state.stage_target_level = 0.0;
                            state.stage_duration = self.release;
                        }
                        EnvelopeType::ASR => {
                            state.stage = EnvelopeStage::Sustain;
                            state.stage_start_time = time;
                            state.stage_start_level = 1.0;
                            state.stage_target_level = self.sustain;
                            state.stage_duration = 0.0;
                        }
                        EnvelopeType::AHDSR => {
                            state.stage = EnvelopeStage::Hold;
                            state.stage_start_time = time;
                            state.stage_start_level = 1.0;
                            state.stage_target_level = 1.0;
                            state.stage_duration = self.hold;
                        }
                    }
                } else {
                    let t = elapsed / self.attack;
                    state.level = state.stage_start_level
                        + (state.stage_target_level - state.stage_start_level)
                            * self.apply_curve(t);
                }
            }

            EnvelopeStage::Hold => {
                if elapsed >= self.hold {
                    state.stage = EnvelopeStage::Decay;
                    state.stage_start_time = time;
                    state.stage_start_level = 1.0;
                    state.stage_target_level = self.sustain;
                    state.stage_duration = self.decay;
                } else {
                    state.level = 1.0;
                }
            }

            EnvelopeStage::Decay => {
                if elapsed >= self.decay {
                    state.stage = EnvelopeStage::Sustain;
                    state.level = self.sustain;
                } else {
                    let t = elapsed / self.decay;
                    state.level = state.stage_start_level
                        + (state.stage_target_level - state.stage_start_level)
                            * self.apply_curve(t);
                }
            }

            EnvelopeStage::Sustain => {
                state.level = self.sustain;
            }

            EnvelopeStage::Release => {
                if elapsed >= self.release {
                    state.stage = EnvelopeStage::Off;
                    state.level = 0.0;
                } else {
                    let t = elapsed / self.release;
                    state.level = state.stage_start_level
                        + (state.stage_target_level - state.stage_start_level)
                            * self.apply_curve(t);
                }
            }

            EnvelopeStage::Off => {
                state.level = 0.0;
            }
        }
    }
}

/// Control action for an envelope automaton.
///
/// Use `GateOn` to trigger the attack phase and `GateOff` to start the release.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
pub enum EnvelopeAction {
    #[default]
    /// No action — the envelope continues its current stage.
    None,
    /// Start the attack phase.
    GateOn,
    /// Start the release phase.
    GateOff,
}

impl Automaton for EnvelopeAutomaton {
    type State = EnvelopeState;
    type Action = EnvelopeAction;

    fn step(
        &self,
        time: Time,
        action: &Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>) {
        let mut new_state = state.clone();

        match action {
            EnvelopeAction::GateOn => {
                new_state.gate = true;
                new_state.stage = EnvelopeStage::Attack;
                new_state.stage_start_time = time;
                new_state.stage_start_level = new_state.level;
                new_state.stage_target_level = 1.0;
            }
            EnvelopeAction::GateOff => {
                new_state.gate = false;
                new_state.stage = EnvelopeStage::Release;
                new_state.stage_start_time = time;
                new_state.stage_start_level = new_state.level;
                new_state.stage_target_level = 0.0;
            }
            EnvelopeAction::None => {}
        }

        self.update_stage(&mut new_state, time);

        let value = self.range.denormalize(new_state.level);

        (new_state, Some(value))
    }

    fn initial_state(&self) -> Self::State {
        EnvelopeState {
            stage: EnvelopeStage::Off,
            level: 0.0,
            stage_start_time: 0.0,
            stage_start_level: 0.0,
            stage_target_level: 0.0,
            stage_duration: 0.0,
            gate: false,
        }
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn extract_value(&self, state: &Self::State) -> f64 {
        self.range.denormalize(state.level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adsr_envelope() {
        let env = EnvelopeAutomaton::adsr("ADSR", 0.1, 0.2, 0.7, 0.3);
        let mut state = env.initial_state();

        assert_eq!(state.stage, EnvelopeStage::Off);

        let (_s, _value) = env.step(0.0, &EnvelopeAction::GateOn, &state);
        state = _s;
        assert_eq!(state.stage, EnvelopeStage::Attack);

        let (s, value) = env.step(0.05, &EnvelopeAction::None, &state);
        state = s;
        assert!(value.unwrap() > 0.0);
        assert!(value.unwrap() < 1.0);

        let (s, _) = env.step(0.5, &EnvelopeAction::GateOff, &state);
        assert_eq!(s.stage, EnvelopeStage::Release);
    }
}
