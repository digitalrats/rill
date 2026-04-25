//! # Огибающие (Envelope) автоматы
//!
//! Генераторы огибающих для управления амплитудой, фильтрами и другими
//! параметрами во времени. Поддерживаются ADSR, AR, ASR и другие типы.

use super::{Automaton, Time, Range, SyncMode, NoAction};

/// Тип огибающей
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeType {
    /// ADSR: Attack, Decay, Sustain, Release
    ADSR,
    /// AR: Attack, Release (для перкуссии)
    AR,
    /// ASR: Attack, Sustain, Release (для органных звуков)
    ASR,
    /// AHDSR: Attack, Hold, Decay, Sustain, Release
    AHDSR,
}

/// Стадия огибающей
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EnvelopeStage {
    Attack,
    Hold,
    Decay,
    Sustain,
    Release,
    Off,
}

impl EnvelopeStage {
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

/// Состояние огибающей
#[derive(Debug, Clone)]
pub struct EnvelopeState {
    /// Текущая стадия
    pub stage: EnvelopeStage,
    /// Текущий уровень (0.0 - 1.0)
    pub level: f64,
    /// Время начала текущей стадии
    pub stage_start_time: Time,
    /// Уровень в начале стадии
    pub stage_start_level: f64,
    /// Целевой уровень стадии
    pub stage_target_level: f64,
    /// Длительность стадии в секундах
    pub stage_duration: f64,
    /// Запущена ли огибающая
    pub gate: bool,
}

/// Действия для огибающей
#[derive(Debug, Clone, Default)]
pub enum EnvelopeAction {
    /// Нет действия
    #[default]
    None,
    /// Запустить огибающую (gate on)
    Trigger,
    /// Отпустить огибающую (gate off)
    Release,
    /// Сбросить в ноль
    Reset,
    /// Установить параметры
    SetParams(EnvelopeParams),
}

/// Параметры огибающей
#[derive(Debug, Clone)]
pub struct EnvelopeParams {
    pub attack: f64,
    pub hold: Option<f64>,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
}

/// Огибающая автомат
#[derive(Debug, Clone)]
pub struct EnvelopeAutomaton {
    /// Имя автомата
    name: String,
    /// Тип огибающей
    env_type: EnvelopeType,
    /// Время атаки (секунды)
    attack: f64,
    /// Время удержания (секунды) - для AHDSR
    hold: f64,
    /// Время спада (секунды)
    decay: f64,
    /// Уровень удержания (0.0 - 1.0)
    sustain: f64,
    /// Время отпускания (секунды)
    release: f64,
    /// Диапазон выходных значений
    range: Range,
    /// Кривая стадий (1.0 = линейная, >1.0 = экспоненциальная, <1.0 = логарифмическая)
    curve: f64,
}

impl EnvelopeAutomaton {
    /// Создать новую ADSR огибающую
    pub fn adsr(
        name: &str,
        attack: f64,
        decay: f64,
        sustain: f64,
        release: f64,
    ) -> Self {
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
    
    /// Создать новую AR огибающую (для перкуссии)
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
    
    /// Создать новую ASR огибающую (для органных звуков)
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
    
    /// Создать новую AHDSR огибающую
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
    
    /// Установить кривую стадий
    pub fn with_curve(mut self, curve: f64) -> Self {
        self.curve = curve.max(0.1);
        self
    }
    
    /// Установить диапазон
    pub fn with_range(mut self, range: Range) -> Self {
        self.range = range;
        self
    }
    
    /// Вычислить значение с учётом кривой
    fn apply_curve(&self, t: f64) -> f64 {
        if self.curve == 1.0 {
            t
        } else {
            t.powf(self.curve)
        }
    }
    
    /// Обновить стадию на основе времени
    fn update_stage(
        &self,
        state: &mut EnvelopeState,
        time: Time,
    ) {
        let elapsed = time - state.stage_start_time;
        
        match state.stage {
            EnvelopeStage::Attack => {
                if elapsed >= self.attack {
                    // Переход к следующей стадии
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
                    state.level = state.stage_start_level + 
                        (state.stage_target_level - state.stage_start_level) * self.apply_curve(t);
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
                    state.level = state.stage_start_level + 
                        (state.stage_target_level - state.stage_start_level) * self.apply_curve(t);
                }
            }
            
            EnvelopeStage::Sustain => {
                state.level = self.sustain;
            }
            
            EnvelopeStage::Release => {
                if elapsed >= self.release || !state.gate {
                    state.stage = EnvelopeStage::Off;
                    state.level = 0.0;
                } else {
                    let t = elapsed / self.release;
                    state.level = state.stage_start_level + 
                        (state.stage_target_level - state.stage_start_level) * self.apply_curve(t);
                }
            }
            
            EnvelopeStage::Off => {
                state.level = 0.0;
            }
        }
    }
}

impl Automaton for EnvelopeAutomaton {
    type State = EnvelopeState;
    type Action = EnvelopeAction;
    
    fn step(
        &self,
        time: Time,
        action: Self::Action,
        state: &Self::State,
    ) -> (Self::State, Option<f64>, Option<Self::Action>) {
        let mut new_state = state.clone();
        
        // Обрабатываем действия
        match action {
            EnvelopeAction::Trigger => {
                new_state.gate = true;
                new_state.stage = EnvelopeStage::Attack;
                new_state.stage_start_time = time;
                new_state.stage_start_level = 0.0;
                new_state.stage_target_level = 1.0;
                new_state.stage_duration = self.attack;
            }
            
            EnvelopeAction::Release => {
                if new_state.gate {
                    new_state.gate = false;
                    new_state.stage = EnvelopeStage::Release;
                    new_state.stage_start_time = time;
                    new_state.stage_start_level = new_state.level;
                    new_state.stage_target_level = 0.0;
                    new_state.stage_duration = self.release;
                }
            }
            
            EnvelopeAction::Reset => {
                new_state.gate = false;
                new_state.stage = EnvelopeStage::Off;
                new_state.level = 0.0;
            }
            
            EnvelopeAction::SetParams(params) => {
                let mut new_automaton = self.clone();
                new_automaton.attack = params.attack;
                if let Some(h) = params.hold {
                    new_automaton.hold = h;
                }
                new_automaton.decay = params.decay;
                new_automaton.sustain = params.sustain;
                new_automaton.release = params.release;
                // В реальном коде нужно обновить автомат
            }
            
            EnvelopeAction::None => {}
        }
        
        // Обновляем стадию
        self.update_stage(&mut new_state, time);
        
        let value = self.range.denormalize(new_state.level);
        
        (new_state, Some(value), None)
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
        let state = env.initial_state();
        
        // Trigger
        let (state, value, _) = env.step(0.0, EnvelopeAction::Trigger, &state);
        assert!(value.unwrap() > 0.0);
        
        // Attack phase
        let (state, value, _) = env.step(0.05, EnvelopeAction::None, &state);
        assert!(value.unwrap() > 0.5);
        
        // Release
        let (state, value, _) = env.step(0.5, EnvelopeAction::Release, &state);
        assert!(value.unwrap() < 0.7);
    }
}