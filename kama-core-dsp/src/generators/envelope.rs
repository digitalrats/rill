//! Генераторы огибающих (ADSR, AR, ASR)

use super::Generator;
use crate::algorithm::{Algorithm, AlgorithmCategory, AlgorithmMetadata};
use crate::math::Smoother;
use crate::vector::prelude::*;
use kama_core::AudioNum;

/// Стадия огибающей
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeStage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

impl EnvelopeStage {
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

/// Тип огибающей
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeType {
    ADSR, // Attack, Decay, Sustain, Release
    AR,   // Attack, Release
    ASR,  // Attack, Sustain, Release
}

/// Генератор огибающей ADSR
pub struct EnvelopeGenerator<T: AudioNum> {
    /// Тип огибающей
    env_type: EnvelopeType,
    /// Времена (в секундах)
    attack: f32,
    decay: f32,
    sustain: ScalarVector1<T>,
    release: f32,
    /// Текущая стадия
    stage: EnvelopeStage,
    /// Текущий уровень
    level: ScalarVector1<T>,
    /// Сглаживатель для устранения щелчков
    smoother: Smoother<T>,
    /// Счётчики в семплах
    attack_samples: usize,
    decay_samples: usize,
    release_samples: usize,
    position: usize,
    /// Частота дискретизации
    sample_rate: f32,
    /// Запущена ли
    gate: bool,
}

impl<T: AudioNum> EnvelopeGenerator<T> {
    /// Создать новую ADSR огибающую
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

    /// Создать новую AR огибающую (для перкуссии)
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

    /// Создать новую ASR огибающую (для орга́нных звуков)
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

    /// Запустить огибающую (gate on)
    pub fn trigger(&mut self) {
        self.gate = true;
        self.stage = EnvelopeStage::Attack;
        self.position = 0;
    }

    /// Отпустить огибающую (gate off)
    pub fn release(&mut self) {
        self.gate = false;
        self.stage = EnvelopeStage::Release;
        self.position = 0;
    }

    /// Получить текущую стадию
    pub fn stage(&self) -> EnvelopeStage {
        self.stage
    }

    /// Проверить, активна ли огибающая
    pub fn is_active(&self) -> bool {
        self.stage != EnvelopeStage::Off
    }
}

impl<T: AudioNum> Algorithm<T> for EnvelopeGenerator<T> {
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

    fn process_block(&mut self, input: &[T], output: &mut [T]) {
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
            author: "Kama Audio",
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

impl<T: AudioNum> Generator<T> for EnvelopeGenerator<T> {
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
