//! Envelope automata for amplitude, filter, and parameter modulation over time.
//!
//! Supports ADSR, AR, ASR, and AHDSR envelope types.

use crate::engine::{Automaton, Range, Time};
use rill_core::traits::ParamValue;

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

/// Internal envelope state: (stage, stage_start_time, stage_start_level).
type EnvelopeInternal = (EnvelopeStage, f64, f64);

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

    /// Get the duration for a given stage.
    fn stage_duration(&self, stage: EnvelopeStage) -> f64 {
        match stage {
            EnvelopeStage::Attack => self.attack,
            EnvelopeStage::Hold => self.hold,
            EnvelopeStage::Decay => self.decay,
            EnvelopeStage::Release => self.release,
            EnvelopeStage::Sustain | EnvelopeStage::Off => f64::INFINITY,
        }
    }

    /// Get the target level for a given stage.
    fn stage_target(&self, stage: EnvelopeStage) -> f64 {
        match stage {
            EnvelopeStage::Attack => 1.0,
            EnvelopeStage::Hold => 1.0,
            EnvelopeStage::Decay => self.sustain,
            EnvelopeStage::Sustain => self.sustain,
            EnvelopeStage::Release => 0.0,
            EnvelopeStage::Off => 0.0,
        }
    }

    /// Transition to the next stage after the current one ends.
    fn next_stage(&self, current: EnvelopeStage) -> EnvelopeStage {
        match (current, self.env_type) {
            (EnvelopeStage::Attack, EnvelopeType::ADSR) => EnvelopeStage::Decay,
            (EnvelopeStage::Attack, EnvelopeType::AR) => EnvelopeStage::Release,
            (EnvelopeStage::Attack, EnvelopeType::ASR) => EnvelopeStage::Sustain,
            (EnvelopeStage::Attack, EnvelopeType::AHDSR) => EnvelopeStage::Hold,
            (EnvelopeStage::Hold, _) => EnvelopeStage::Decay,
            (EnvelopeStage::Decay, _) => EnvelopeStage::Sustain,
            (EnvelopeStage::Release, _) => EnvelopeStage::Off,
            (EnvelopeStage::Sustain, _) => EnvelopeStage::Sustain,
            (EnvelopeStage::Off, _) => EnvelopeStage::Off,
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
    type Internal = EnvelopeInternal;
    type Action = EnvelopeAction;

    fn step(
        &self,
        internal: &mut Self::Internal,
        current: &ParamValue,
        time: Time,
        action: &Self::Action,
    ) -> ParamValue {
        let (stage, stage_start_time, stage_start_level) = *internal;
        let current_level = current.as_f32().unwrap_or(0.0) as f64;

        let (new_stage, new_start_time, new_start_level) = match action {
            EnvelopeAction::GateOn => (EnvelopeStage::Attack, time, current_level),
            EnvelopeAction::GateOff => (EnvelopeStage::Release, time, current_level),
            EnvelopeAction::None => (stage, stage_start_time, stage_start_level),
        };

        let elapsed = time - new_start_time;
        let duration = self.stage_duration(new_stage);
        let target = self.stage_target(new_stage);

        let (next_stage, next_start_time, next_start_level, level) = if elapsed >= duration {
            let next = self.next_stage(new_stage);
            let next_target = self.stage_target(next);
            let next_dur = self.stage_duration(next);
            if next_dur.is_infinite() {
                (next, time, next_target, next_target)
            } else {
                // Stage ended exactly — use target level as output, start next stage
                (next, time, target, target)
            }
        } else {
            let t = elapsed / duration;
            let curved = self.apply_curve(t);
            let lvl = new_start_level + (target - new_start_level) * curved;
            (new_stage, new_start_time, new_start_level, lvl)
        };

        *internal = (next_stage, next_start_time, next_start_level);
        let value = self.range.denormalize(level);
        ParamValue::Float(value as f32)
    }

    fn initial_internal(&self) -> Self::Internal {
        (EnvelopeStage::Off, 0.0, 0.0)
    }

    fn name(&self) -> &str {
        &self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adsr_envelope() {
        let env = EnvelopeAutomaton::adsr("ADSR", 0.1, 0.2, 0.7, 0.3);
        let mut internal = env.initial_internal();
        let current = ParamValue::Float(0.0);

        assert_eq!(internal.0, EnvelopeStage::Off);

        let value = env.step(&mut internal, &current, 0.0, &EnvelopeAction::GateOn);
        assert_eq!(internal.0, EnvelopeStage::Attack);

        let value = env.step(&mut internal, &value, 0.05, &EnvelopeAction::None);
        let val = value.as_f32().unwrap();
        assert!(val > 0.0);
        assert!(val < 1.0);

        let _value = env.step(&mut internal, &value, 0.5, &EnvelopeAction::GateOff);
        assert_eq!(internal.0, EnvelopeStage::Release);
    }
}
