//! Генераторы огибающих (ADSR, AR, ASR)

use super::Generator;
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::math::Smoother;
use crate::vector::prelude::*;
use rill_core::traits::{ActionContext, ProcessResult};
use rill_core::Transcendental;

/// Current stage of an envelope generator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeStage {
    /// Attack phase — level rises from 0 to 1.
    Attack,
    /// Decay phase — level falls from 1 to sustain level.
    Decay,
    /// Sustain phase — level holds at the sustain value.
    Sustain,
    /// Release phase — level falls from sustain to 0.
    Release,
    /// Off — level is 0 and the envelope is inactive.
    Off,
}

impl EnvelopeStage {
    /// Human-readable name of the current stage.
    pub fn name(&self) -> &'static str {
        match self {
            EnvelopeStage::Attack => "Attack",
            EnvelopeStage::Decay => "Decay",
            EnvelopeStage::Sustain => "Sustain",
            EnvelopeStage::Release => "Release",
            EnvelopeStage::Off => "Off",
        }
    }
}

/// Envelope shape variant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeType {
    /// Attack, Decay, Sustain, Release — the classic four-stage envelope.
    ADSR,
    /// Attack, Release — no sustain, for percussive sounds.
    AR,
    /// Attack, Sustain, Release — sustain holds until gate-off, for organ sounds.
    ASR,
}

/// Multi-stage envelope generator (ADSR, AR, ASR).
///
/// Processes an audio-rate gate signal: values > 0.5 trigger the attack
/// phase, values ≤ 0.5 trigger the release phase. Output ramps smoothly
/// between stages.
pub struct EnvelopeGenerator<T: Transcendental> {
    env_type: EnvelopeType,
    attack: f32,
    decay: f32,
    sustain: ScalarVector1<T>,
    release: f32,
    stage: EnvelopeStage,
    level: ScalarVector1<T>,
    smoother: Smoother<T>,
    attack_samples: usize,
    decay_samples: usize,
    release_samples: usize,
    position: usize,
    sample_rate: f32,
    gate: bool,
}

impl<T: Transcendental> EnvelopeGenerator<T> {
    /// Create a new ADSR envelope.
    ///
    /// * `attack` — attack time in seconds.
    /// * `decay` — decay time in seconds.
    /// * `sustain` — sustain level (0..1).
    /// * `release` — release time in seconds.
    pub fn adsr(attack: f32, decay: f32, sustain: T, release: f32) -> Self {
        Self {
            env_type: EnvelopeType::ADSR,
            attack,
            decay,
            sustain: ScalarVector1::splat(sustain),
            release,
            stage: EnvelopeStage::Off,
            level: ScalarVector1::splat(T::ZERO),
            smoother: Smoother::new(T::from_f32(0.5)),
            attack_samples: 0,
            decay_samples: 0,
            release_samples: 0,
            position: 0,
            sample_rate: 44100.0,
            gate: false,
        }
    }

    /// Create a new AR (Attack-Release) envelope for percussive sounds.
    pub fn ar(attack: f32, release: f32) -> Self {
        Self {
            env_type: EnvelopeType::AR,
            attack,
            decay: 0.0,
            sustain: ScalarVector1::splat(T::ZERO),
            release,
            stage: EnvelopeStage::Off,
            level: ScalarVector1::splat(T::ZERO),
            smoother: Smoother::new(T::from_f32(0.5)),
            attack_samples: 0,
            decay_samples: 0,
            release_samples: 0,
            position: 0,
            sample_rate: 44100.0,
            gate: false,
        }
    }

    /// Create a new ASR (Attack-Sustain-Release) envelope for organ-like sounds.
    pub fn asr(attack: f32, sustain: T, release: f32) -> Self {
        Self {
            env_type: EnvelopeType::ASR,
            attack,
            decay: 0.0,
            sustain: ScalarVector1::splat(sustain),
            release,
            stage: EnvelopeStage::Off,
            level: ScalarVector1::splat(T::ZERO),
            smoother: Smoother::new(T::from_f32(0.5)),
            attack_samples: 0,
            decay_samples: 0,
            release_samples: 0,
            position: 0,
            sample_rate: 44100.0,
            gate: false,
        }
    }

    /// Обновить счётчики семплов
    fn update_samples(&mut self) {
        self.attack_samples = (self.attack * self.sample_rate) as usize;
        self.decay_samples = (self.decay * self.sample_rate) as usize;
        self.release_samples = (self.release * self.sample_rate) as usize;
    }

    /// Start the envelope (gate on). Resets to the attack phase.
    pub fn trigger(&mut self) {
        self.gate = true;
        self.stage = EnvelopeStage::Attack;
        self.position = 0;
    }

    /// Release the envelope (gate off). Enters the release phase.
    pub fn release(&mut self) {
        self.gate = false;
        self.stage = EnvelopeStage::Release;
        self.position = 0;
    }

    /// Current envelope stage.
    pub fn stage(&self) -> EnvelopeStage {
        self.stage
    }

    /// Returns `true` if the envelope is not in the `Off` stage.
    pub fn is_active(&self) -> bool {
        self.stage != EnvelopeStage::Off
    }
}

impl<T: Transcendental> Algorithm<T> for EnvelopeGenerator<T> {
    fn init(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.update_samples();
        self.reset();
    }

    fn reset(&mut self) {
        self.stage = EnvelopeStage::Off;
        self.level = ScalarVector1::splat(T::ZERO);
        self.position = 0;
        self.gate = false;
        self.smoother.set_current(T::ZERO);
    }

    fn process(
        &mut self,
        input: Option<&[T]>,
        output: &mut [T],
        _ctx: &ActionContext,
    ) -> ProcessResult<()> {
        let input = input.unwrap_or(&[]);
        let len = input.len().min(output.len());
        for i in 0..len {
            let gate_signal = input[i];
            // Обновляем gate из входного сигнала если есть
            if gate_signal.to_f32() > 0.5 && !self.gate {
                self.trigger();
            } else if gate_signal.to_f32() <= 0.5 && self.gate {
                self.release();
            }

            // Генерация огибающей
            match self.stage {
                EnvelopeStage::Attack => {
                    let target = ScalarVector1::splat(T::from_f32(1.0));
                    self.level = self.level
                        + (target - self.level)
                            * ScalarVector1::splat(T::from_f32(1.0 / self.attack_samples as f32));
                    self.position += 1;

                    if self.position >= self.attack_samples {
                        match self.env_type {
                            EnvelopeType::ADSR => self.stage = EnvelopeStage::Decay,
                            EnvelopeType::AR => self.stage = EnvelopeStage::Release,
                            EnvelopeType::ASR => self.stage = EnvelopeStage::Sustain,
                        }
                        self.position = 0;
                    }
                }

                EnvelopeStage::Decay => {
                    let target = self.sustain;
                    self.level = self.level
                        + (target - self.level)
                            * ScalarVector1::splat(T::from_f32(1.0 / self.decay_samples as f32));
                    self.position += 1;

                    if self.position >= self.decay_samples {
                        self.stage = EnvelopeStage::Sustain;
                        self.position = 0;
                    }
                }

                EnvelopeStage::Sustain => {
                    // Просто держим уровень
                    self.level = self.sustain;
                }

                EnvelopeStage::Release => {
                    let target = ScalarVector1::splat(T::ZERO);
                    self.level = self.level
                        + (target - self.level)
                            * ScalarVector1::splat(T::from_f32(1.0 / self.release_samples as f32));
                    self.position += 1;

                    if self.position >= self.release_samples {
                        self.stage = EnvelopeStage::Off;
                        self.position = 0;
                    }
                }

                EnvelopeStage::Off => {
                    self.level = ScalarVector1::splat(T::ZERO);
                }
            }

            // Применяем сглаживание
            output[i] = self.smoother.process_sample(self.level.extract(0));
        }
        Ok(())
    }

    fn metadata(&self) -> AlgorithmMetadata {
        AlgorithmMetadata {
            name: match self.env_type {
                EnvelopeType::ADSR => "ADSR Envelope",
                EnvelopeType::AR => "AR Envelope",
                EnvelopeType::ASR => "ASR Envelope",
            },
            category: AlgorithmCategory::Generator,
            description: match self.env_type {
                EnvelopeType::ADSR => "Attack-Decay-Sustain-Release envelope generator",
                EnvelopeType::AR => "Attack-Release envelope generator (percussion)",
                EnvelopeType::ASR => "Attack-Sustain-Release envelope generator (organ)",
            },
            author: "Rill",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: Transcendental> Generator<T> for EnvelopeGenerator<T> {
    fn phase(&self) -> T {
        T::ZERO
    }
    fn set_phase(&mut self, _phase: T) {}
    fn frequency(&self) -> f32 {
        0.0
    }
    fn set_frequency(&mut self, _freq: f32) {}
    fn amplitude(&self) -> T {
        self.level.extract(0)
    }
    fn set_amplitude(&mut self, _amp: T) {}
}
